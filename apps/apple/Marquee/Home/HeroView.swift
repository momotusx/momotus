import SwiftUI

/// The home hero, Apple's top shelf: one full-bleed backdrop under its ramps.
/// Wide screens (the Mac, TV) a left column as tv.apple.com draws it: the
/// logo, the genre line, the meta line with the ratings, two lines of
/// synopsis, the core's reason on the explainability line, Play and Details.
/// The phone the iPhone's centered column: the logo, the genre line with the
/// ratings, the reason, the 178x48 pill and the 48 pt glass Details. Holds
/// its height while empty so rows never jump.
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

    var body: some View {
        // The art is a minimum, the words decide: at a large text size or in a
        // short window the hero grows to its words instead of cutting them.
        Group {
            if let hero {
                if metrics.wide { wide(hero) } else { phone(hero) }
            } else {
                Color.clear
            }
        }
        .heroFrame(height: height, alignment: metrics.wide ? .bottomLeading : .bottom) {
            // The iPhone's large title at the hero's top, over the art's top wash, also while it loads.
            if !metrics.wide {
                Text("Home")
                    .display(34)
                    .foregroundStyle(Theme.Palette.text)
                    .accessibilityAddTraits(.isHeader)
                    .padding(.leading, 16)
                    .padding(.top, 2)
            }
        }
        .background {
            // The phone's art runs up under the status bar; the Mac's starts under the black header.
            ZStack {
                ArtImage(url: hero?.tile.background ?? hero?.tile.poster, maxPixels: ArtSize.backdrop)
                    .accessibilityHidden(true)
                scrims
            }
            .ignoresSafeArea(edges: metrics.wide ? [] : .top)
        }
    }

    /// The ramps (BackdropStops), so every word and secondary reads on any picture. Wide, bottom
    /// first as the frames layer them: the vignette, the 80 degree wash, the foot into the page.
    private var scrims: some View {
        let bg = Theme.Palette.bg
        return ZStack {
            if metrics.wide {
                GeometryReader { geo in
                    ZStack {
                        BackdropStops.vignetteGradient
                        BackdropStops.wash(geo.size)
                        LinearGradient(stops: BackdropStops.gradient(BackdropStops.heroFoot, bg), startPoint: .bottom, endPoint: .top)
                    }
                }
            } else {
                GeometryReader { geo in
                    ZStack {
                        LinearGradient(stops: BackdropStops.gradient(BackdropStops.heroPhoneTop, bg), startPoint: .top, endPoint: .bottom)
                        BackdropStops.phoneFoot(geo.size, bg)
                    }
                }
            }
        }
        .allowsHitTesting(false)
    }

    private func wide(_ hero: Hero) -> some View {
        let tile = hero.tile
        return VStack(alignment: .leading, spacing: Self.gap) {
            TitleLogo(logo: tile.logo, name: tile.name, maxWidth: Self.logoWidth, height: Self.logoHeight, nameSize: Theme.type.hero + 12)
            Text(genreLine(hero))
                .textFont(Self.genre, weight: .semibold, relativeTo: .callout)
                .foregroundStyle(Theme.Palette.text)
            MetaLine(ratings: ratingParts(tile.ratings), parts: [tile.releaseInfo, hero.runtime].compactMap { $0 }, size: Self.meta)
                .foregroundStyle(Theme.Palette.dim)
            if let description = hero.description {
                Text(description)
                    .textFont(Self.genre, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.muted)
                    .lineLimit(2)
                    .frame(maxWidth: Self.synopsisWidth, alignment: .leading)
                    .fixedSize(horizontal: false, vertical: true)
            }
            reason(hero)
            HStack(spacing: 12) {
                playButton(tile)
                NavigationLink(value: TitleRoute(tile)) { Label("Details", glyph: "info", size: Self.icon) }
                    .buttonStyle(PillButtonStyle())
                    .modifier(StartFocus(scope: tile.playVideoId == nil ? startScope : nil))
            }
            .padding(.top, 16 - Self.gap)
        }
        .frame(maxWidth: 900, alignment: .leading)
        .padding(.leading, metrics.gutter)
        .padding(.trailing, metrics.gutter)
        .padding(.bottom, Self.foot)
    }

    private func phone(_ hero: Hero) -> some View {
        let tile = hero.tile
        return VStack(spacing: 0) {
            TitleLogo(logo: tile.logo, name: tile.name, maxWidth: 300, height: 86, nameSize: Theme.type.title1, alignment: .bottom)
            // One line as the iPhone has it; the ratings go under the genres when the pair is too wide.
            let genre = Text(genreLine(hero)).textFont(15, weight: .semibold, relativeTo: .callout).foregroundStyle(Theme.Palette.text)
            let ratings = MetaLine(ratings: ratingParts(tile.ratings), parts: [], size: 13)
            ViewThatFits(in: .horizontal) {
                HStack(spacing: 8) { genre.fixedSize(); ratings.fixedSize() }
                VStack(spacing: 4) { genre; ratings }
            }
            .padding(.top, 12)
            reason(hero).padding(.top, 6)
            HStack(spacing: 12) {
                playButton(tile).frame(width: 178)
                NavigationLink(value: TitleRoute(tile)) {
                    Glyph("info", size: 22)
                        // The glyph scales with the text, held so it stays inside its 48 pt circle.
                        .dynamicTypeSize(...DynamicTypeSize.xxxLarge)
                        .foregroundStyle(.white)
                        .frame(width: 48, height: 48)
                        .background(SecondaryFill(shape: Circle()))
                }
                .buttonStyle(FocusPlainStyle(shape: Circle(), scale: Theme.Focus.scaleCard))
                .accessibilityLabel("Details")
            }
            .padding(.top, 16)
        }
        .multilineTextAlignment(.center)
        .padding(.horizontal, 20)
        .padding(.bottom, 56)
    }

    /// The core's reason on Apple's explainability line, only when it sends one.
    @ViewBuilder private func reason(_ hero: Hero) -> some View {
        if !hero.reason.isEmpty {
            Text(hero.reason)
                .textFont(Self.reasonSize, weight: .semibold, relativeTo: .caption)
                .foregroundStyle(Theme.Palette.text)
                .lineLimit(2)
        }
    }

    @ViewBuilder private func playButton(_ tile: Tile) -> some View {
        // Play plays, as on the web and Android; a series with no video named has none.
        if tile.playVideoId != nil {
            Button { play(tile) } label: {
                Group {
                    if starting {
                        HStack(spacing: 10) {
                            ProgressView().controlSize(.small).tint(Theme.Palette.onPrimary)
                            Text("Starting")
                        }
                    } else {
                        Label("Play", glyph: "play-fill", size: Self.icon)
                    }
                }
                .frame(maxWidth: metrics.wide ? nil : .infinity)
                // The iPhone's pill is 178 wide: a large text size shrinks the word rather than cut it.
                .lineLimit(1)
                .minimumScaleFactor(0.7)
            }
            .buttonStyle(PillButtonStyle(prominent: true))
            // Never off while starting: a TV moves focus off a disabled button,
            // and `playHero` ignores a second press.
            .modifier(StartFocus(scope: startScope))
        }
    }

    /// "Movie · Drama · Thriller": the kind, then the core's genres.
    private func genreLine(_ hero: Hero) -> String {
        let kind = hero.tile.kind == "movie" ? "Movie" : hero.tile.kind == "series" ? "Series" : hero.tile.kind.capitalized
        return ([kind] + (hero.genres ?? []).prefix(2)).joined(separator: " \u{B7} ")
    }

    // tv.apple.com's sizes on the Mac, the tvOS scale on TV.
    #if os(tvOS)
    private static let gap: CGFloat = 12
    private static let logoWidth: CGFloat = 560
    private static let logoHeight: CGFloat = 140
    private static let genre = Theme.type.callout
    private static let meta = Theme.type.caption
    private static let reasonSize = Theme.type.caption
    private static let synopsisWidth: CGFloat = 820
    private static let foot: CGFloat = 75
    private static let icon = Theme.type.body + 1
    #else
    private static let gap: CGFloat = 9
    private static let logoWidth: CGFloat = 400
    private static let logoHeight: CGFloat = 81
    private static let genre: CGFloat = 15
    private static let meta: CGFloat = 13
    private static let reasonSize: CGFloat = 13
    private static let synopsisWidth: CGFloat = 560
    private static let foot: CGFloat = 75
    #if os(macOS)
    // The desktop frames' 16 px icon in the 40 pt pill; the iPhone keeps its 18.
    private static let icon: CGFloat = 16
    #else
    private static let icon = Theme.type.body + 1
    #endif
    #endif
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

/// "2011–2019 · Drama, Fantasy · 8 seasons · IMDb 9.2 · RT 89%": the year
/// first, the parts joined by dots, then the ratings row. Lines break only
/// between the parts. `imdbLink` makes IMDb's score open its page (never on TV).
struct MetaLine: View {
    let ratings: [String]
    let parts: [String]
    let size: CGFloat
    var imdbLink: URL? = nil
    @Environment(\.openURL) private var openURL

    var body: some View {
        FlowRow(spacing: size * 0.45) {
            ForEach(Array((parts + ratings).enumerated()), id: \.offset) { index, part in
                if index > 0 {
                    Text("\u{B7}").foregroundStyle(Theme.Palette.dim)
                }
                if index >= parts.count {
                    score(part, link: index == parts.count ? link : nil)
                } else {
                    Text(part)
                }
            }
        }
        .textFont(size, weight: .semibold, relativeTo: .callout)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel((parts + ratings.map(spokenScore)).joined(separator: ", "))
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
        // One line, never cut: the row breaks between parts, never inside a score. The ratings row
        // as plain text: the source at 70 percent, the value in white at 600 (D10).
        let label = ratingText(text).fontWeight(.regular)
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

/// One score as the ratings row writes it (D10): the source's name at 70 percent and its value in
/// white at 600, plain text with no star and no logo ("IMDb 8.4", "RT 98%", "RT Fresh").
func ratingText(_ score: String) -> Text {
    let parts = score.split(separator: " ", maxSplits: 1).map(String.init)
    guard parts.count == 2 else { return Text(score).fontWeight(.semibold).foregroundStyle(.white) }
    return Text(parts[0]).foregroundStyle(Theme.Palette.muted) + Text(" ") + Text(parts[1]).fontWeight(.semibold).foregroundStyle(.white)
}

/// The title's logo art, or its name set in the display face when there is none.
struct TitleLogo: View {
    let logo: String?
    let name: String
    let maxWidth: CGFloat
    let height: CGFloat
    /// Size of the name when there is no logo.
    var nameSize = Theme.type.hero
    /// Where the art sits in its box: leading on wide screens, centered on the phone.
    var alignment: Alignment = .bottomLeading

    /// nil while loading; then the art, or none when there is no logo.
    @State private var loaded: DecodedArt??

    var body: some View {
        ZStack(alignment: alignment) {
            switch loaded {
            case .some(.some(let art)):
                Image(decorative: art.image, scale: 1)
                    .resizable()
                    .scaledToFit()
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: alignment)
                    .transition(.opacity)
            case .some(.none):
                Text(name)
                    .display(nameSize)
                    .foregroundStyle(Theme.Palette.text)
                    .lineLimit(2)
                    .minimumScaleFactor(0.6)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: alignment)
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
        .frame(maxWidth: maxWidth, maxHeight: height, alignment: alignment)
        .frame(height: height, alignment: alignment)
        .accessibilityElement()
        .accessibilityLabel(name)
        .accessibilityAddTraits(.isHeader)
    }
}
