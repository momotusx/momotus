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
                ArtImage(url: hero?.tile.background ?? hero?.tile.plainPoster, maxPixels: ArtSize.backdrop)
                    .accessibilityHidden(true)
                HeroScrims(wide: metrics.wide, page: Theme.Palette.bg)
            }
            .ignoresSafeArea(edges: metrics.wide ? [] : .top)
        }
    }

    private func wide(_ hero: Hero) -> some View {
        let tile = hero.tile
        return VStack(alignment: .leading, spacing: HeroLayout.gap) {
            TitleLogo(logo: tile.logo, name: tile.name, maxWidth: HeroLayout.logoWidth, height: HeroLayout.logoHeight, nameSize: Theme.type.hero + 12)
            Text(genreLine(hero))
                .textFont(HeroLayout.genre, weight: .semibold, relativeTo: .callout)
                .foregroundStyle(Theme.Palette.text)
            MetaLine(ratings: ratingParts(tile.ratings), parts: [tile.releaseInfo, hero.runtime].compactMap { $0 }, size: HeroLayout.meta)
                .foregroundStyle(Theme.Palette.dim)
            if let description = hero.description {
                Text(description)
                    .textFont(HeroLayout.genre, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.muted)
                    .lineLimit(2)
                    .frame(maxWidth: HeroLayout.synopsisWidth, alignment: .leading)
                    .fixedSize(horizontal: false, vertical: true)
            }
            reason(hero)
            HStack(spacing: 12) {
                playButton(tile)
                NavigationLink(value: TitleRoute(tile)) { Label("Details", glyph: "info", size: HeroLayout.icon) }
                    .buttonStyle(PillButtonStyle())
                    .modifier(StartFocus(scope: tile.playVideoId == nil ? startScope : nil))
            }
            .padding(.top, 16 - HeroLayout.gap)
        }
        .frame(maxWidth: 900, alignment: .leading)
        .padding(.leading, metrics.gutter)
        .padding(.trailing, metrics.gutter)
        .padding(.bottom, HeroLayout.foot)
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
                .textFont(HeroLayout.reasonSize, weight: .semibold, relativeTo: .caption)
                .foregroundStyle(Theme.Palette.text)
                .lineLimit(2)
        }
    }

    @ViewBuilder private func playButton(_ tile: Tile) -> some View {
        // Play plays, as on the web and Android; a series with no video named has none.
        if tile.playVideoId != nil {
            Button { play(tile) } label: {
                Label("Play", glyph: "play-fill", size: HeroLayout.icon)
                .frame(maxWidth: metrics.wide ? nil : .infinity)
                // The iPhone's pill is 178 wide: a large text size shrinks the word rather than cut it.
                .lineLimit(1)
                .minimumScaleFactor(0.7)
            }
            .buttonStyle(PillButtonStyle(prominent: true))
            .modifier(StartFocus(scope: startScope))
        }
    }

    /// "Movie · Drama · Thriller": the kind, then the core's genres.
    private func genreLine(_ hero: Hero) -> String {
        return ([kindWord(hero.tile.kind)].compactMap { $0 } + (hero.genres ?? []).prefix(2)).joined(separator: " \u{B7} ")
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
