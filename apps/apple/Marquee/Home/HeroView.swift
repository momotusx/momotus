import SwiftUI

/// The home hero: an inset rounded card holding the backdrop, why the title
/// is here, its logo (or its name), the meta line, three lines of synopsis,
/// Play and Details. Holds its height while empty so rows never jump.
struct HeroView: View {
    let hero: Hero?
    let height: CGFloat
    /// TV: the scope whose default focus is Play (Back on Home returns there).
    var startScope: Namespace.ID?
    /// Plays the hero's video, as the title page's Play does.
    var play: (Tile) -> Void = { _ in }
    /// Play was pressed and its pick is settling.
    var starting = false
    @Environment(\.metrics) private var metrics

    /// The explainability line: the caption on TV, the frames' 13/600 elsewhere.
    #if os(tvOS)
    private static let reasonSize = Theme.type.caption
    #else
    private static let reasonSize: CGFloat = 13
    #endif

    var body: some View {
        let card = RoundedRectangle(cornerRadius: Theme.Radius.hero, style: .continuous)
        // The art is a minimum, the words decide: at a large text size or in a
        // short window the card grows to its words instead of cutting them.
        Group {
            if let hero {
                details(hero)
                    .padding(.leading, metrics.wide ? Theme.Space.xl - 12 : 18)
                    .padding(.trailing, metrics.wide ? Theme.Space.l : 18)
                    .padding(.bottom, metrics.wide ? Theme.Space.l : 20)
                    // Clear of the phone's wordmark at the card's top.
                    .padding(.top, metrics.wide ? Theme.Space.l : 56)
            } else {
                Color.clear
            }
        }
        .frame(maxWidth: .infinity, minHeight: height, alignment: .bottomLeading)
        .background {
            ArtImage(url: hero?.tile.background ?? hero?.tile.poster, maxPixels: ArtSize.backdrop)
                .accessibilityHidden(true)
            scrims
        }
        .clipShape(card)
        .overlay { card.strokeBorder(Theme.Palette.edge, lineWidth: 1).allowsHitTesting(false) }
        .overlay(alignment: .topLeading) {
            // Phone: the wordmark sits in the card; wide screens carry it in the navigation.
            if !metrics.wide {
                Wordmark(size: 22)
                    .shadow(color: Theme.Palette.bg.opacity(0.6), radius: 6, y: 1)
                    .padding(.leading, 18).padding(.top, 16)
            }
        }
        .padding(.horizontal, metrics.wide ? metrics.gutter - 16 : 10)
    }

    /// The art fades to the page colour from the left (wide) or the top and
    /// the bottom (phone), so the words read on any picture.
    private var scrims: some View {
        let bg = Theme.Palette.bg
        return ZStack {
            if metrics.wide {
                LinearGradient(stops: BackdropStops.gradient(BackdropStops.heroSide, bg), startPoint: .leading, endPoint: .trailing)
                LinearGradient(stops: BackdropStops.gradient(BackdropStops.heroFoot, bg), startPoint: .bottom, endPoint: .top)
            } else {
                LinearGradient(stops: BackdropStops.gradient(BackdropStops.heroPhoneTop(height: height), bg), startPoint: .top, endPoint: .bottom)
                LinearGradient(stops: BackdropStops.gradient(BackdropStops.heroPhoneFoot, bg), startPoint: .bottom, endPoint: .top)
            }
        }
        .allowsHitTesting(false)
    }

    private func details(_ hero: Hero) -> some View {
        let tile = hero.tile
        let wide = metrics.wide
        return VStack(alignment: .leading, spacing: 0) {
            // The core's reason as Apple's explainability line: no eyebrow, no chip.
            Text(hero.reason)
                .textFont(Self.reasonSize, weight: .semibold, relativeTo: .caption)
                .foregroundStyle(Theme.Palette.text)
                .lineLimit(2)
                .padding(.bottom, wide ? 14 : 10)
            TitleLogo(
                logo: tile.logo, name: tile.name, maxWidth: wide ? 440 : 260, height: wide ? 120 : 84,
                nameSize: wide ? Theme.type.hero + 12 : Theme.type.title1)
                .padding(.bottom, wide ? 18 : 12)
            MetaLine(ratings: ratingParts(tile.ratings), parts: heroMeta(hero), size: wide ? Theme.type.callout : 13)
                .foregroundStyle(Theme.Palette.muted)
                .padding(.bottom, wide ? 14 : 10)
            if let description = hero.description {
                Text(description)
                    .textFont(wide ? Theme.type.callout : 14, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.muted)
                    .lineLimit(3)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.bottom, wide ? 22 : 16)
            }
            HStack(alignment: .top, spacing: wide ? 24 : 14) {
                // Play plays, as on the web and Android; a series with no video named has none.
                if tile.playVideoId != nil {
                    Button { play(tile) } label: {
                        if starting {
                            HStack(spacing: 10) {
                                ProgressView().controlSize(.small).tint(Theme.Palette.onPrimary)
                                Text("Starting")
                            }
                        } else {
                            Label("Play", glyph: "play-fill")
                        }
                    }
                    .buttonStyle(PillButtonStyle(prominent: true))
                    // Never off while starting: a TV moves focus off a disabled button,
                    // and `playHero` ignores a second press.
                    .modifier(StartFocus(scope: startScope))
                }
                NavigationLink(value: TitleRoute(tile)) {
                    Label("Details", glyph: "info")
                }
                .buttonStyle(RoundButtonStyle())
                .modifier(StartFocus(scope: tile.playVideoId == nil ? startScope : nil))
            }
        }
        .frame(maxWidth: wide ? 620 : .infinity, alignment: .leading)
    }
}

/// TV: a scope's default focus (the hero's Play, or a row's first tile); elsewhere nothing.
struct StartFocus: ViewModifier {
    let scope: Namespace.ID?

    func body(content: Content) -> some View {
        #if os(tvOS)
        if let scope { content.prefersDefaultFocus(true, in: scope) } else { content }
        #else
        content
        #endif
    }
}

/// The hero's meta after the scores: year, runtime and genres, as the core gives them.
func heroMeta(_ hero: Hero) -> [String] {
    let genres = (hero.genres ?? []).joined(separator: ", ")
    return [hero.tile.releaseInfo, hero.runtime, genres.isEmpty ? nil : genres].compactMap { $0 }
}

/// "IMDb 9.2 · RT 89% · 2011–2019 · Drama, Fantasy · 8 seasons": the scores
/// as plain text, the rest joined by dots. Lines break only between the parts.
/// `imdbLink` makes the first score, IMDb's, open its page (never on TV).
struct MetaLine: View {
    let ratings: [String]
    let parts: [String]
    let size: CGFloat
    var imdbLink: URL? = nil
    @Environment(\.openURL) private var openURL

    var body: some View {
        FlowRow(spacing: size * 0.45) {
            ForEach(Array((ratings + parts).enumerated()), id: \.offset) { index, part in
                if index > 0 {
                    Text("\u{B7}").foregroundStyle(Theme.Palette.dim)
                }
                if index < ratings.count {
                    score(part, link: index == 0 ? link : nil)
                } else {
                    Text(part)
                }
            }
        }
        .textFont(size, weight: .semibold, relativeTo: .callout)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel((ratings.map(spokenScore) + parts).joined(separator: ", "))
        .accessibilityActions {
            if let link { Button("Open on IMDb") { openURL(link) } }
        }
    }

    private var link: URL? {
        #if os(tvOS)
        nil
        #else
        imdbLink
        #endif
    }

    @ViewBuilder private func score(_ text: String, link: URL?) -> some View {
        // One line, never cut: the row breaks between parts, never inside a score.
        let label = Text(text).fontWeight(.bold).foregroundStyle(Theme.Palette.text)
            .lineLimit(1).fixedSize()
        if let link {
            Link(destination: link) { label }
                .buttonStyle(.plain)
                #if os(iOS)
                // A finger-sized target.
                .frame(minHeight: 44)
                .contentShape(Rectangle())
                #endif
        } else {
            label
        }
    }
}

/// The title's logo art, or its name set in the display face when there is none.
struct TitleLogo: View {
    let logo: String?
    let name: String
    let maxWidth: CGFloat
    let height: CGFloat
    /// Size of the name when there is no logo.
    var nameSize = Theme.type.hero

    /// nil while loading; then the art, or none when there is no logo.
    @State private var loaded: DecodedArt??

    var body: some View {
        ZStack(alignment: .bottomLeading) {
            switch loaded {
            case .some(.some(let art)):
                Image(decorative: art.image, scale: 1)
                    .resizable()
                    .scaledToFit()
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomLeading)
                    .transition(.opacity)
            case .some(.none):
                Text(name)
                    .display(nameSize)
                    .foregroundStyle(Theme.Palette.text)
                    .lineLimit(2)
                    .minimumScaleFactor(0.6)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomLeading)
            case .none:
                Color.clear
            }
        }
        .task(id: logo) {
            if logo != nil { loaded = nil }
            let start = ContinuousClock.now
            let art = await ArtLoader.shared.image(logo, maxPixels: ArtSize.logo)
            withAnimation(reveal(since: start)) { loaded = .some(art) }
        }
        .frame(maxWidth: maxWidth, maxHeight: height, alignment: .bottomLeading)
        .frame(height: height, alignment: .bottomLeading)
        .accessibilityElement()
        .accessibilityLabel(name)
        .accessibilityAddTraits(.isHeader)
    }
}
