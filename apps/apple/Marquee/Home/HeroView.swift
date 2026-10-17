import SwiftUI

/// Full-bleed art for one title with its logo, the reason it is here and
/// the two actions. Holds its height while empty so rows never jump.
struct HeroView: View {
    let hero: Hero?
    let height: CGFloat
    let overlap: CGFloat
    @Environment(\.metrics) private var metrics

    var body: some View {
        ZStack(alignment: .bottomLeading) {
            ArtImage(url: hero?.tile.background ?? hero?.tile.poster, maxPixels: ArtSize.backdrop)
                .frame(maxWidth: .infinity)
                .frame(height: height)
                .clipped()
                .accessibilityHidden(true)
            scrims
            if let hero {
                details(hero)
                    .padding(.horizontal, metrics.gutter)
                    .padding(.bottom, overlap + Theme.Space.m)
            }
        }
        .frame(height: height)
        .overlay(alignment: .topLeading) {
            if !metrics.wide {
                Text("Marquee")
                    .font(.display(Theme.type.title1))
                    .foregroundStyle(Theme.Palette.text)
                    .padding(.leading, metrics.gutter)
                    .padding(.top, 64)
                    .accessibilityAddTraits(.isHeader)
            }
        }
        .padding(.bottom, -overlap)
    }

    private var scrims: some View {
        let bg = Theme.Palette.bg
        return ZStack {
            if metrics.wide {
                LinearGradient(
                    stops: [.init(color: bg.opacity(0.94), location: 0), .init(color: bg.opacity(0.55), location: 0.38), .init(color: .clear, location: 0.7)],
                    startPoint: .leading, endPoint: .trailing)
            } else {
                // Under the status bar and the wordmark, so both read on bright art.
                LinearGradient(
                    stops: [.init(color: bg.opacity(0.75), location: 0), .init(color: bg.opacity(0.5), location: 0.2), .init(color: .clear, location: 0.34)],
                    startPoint: .top, endPoint: .bottom)
            }
            LinearGradient(
                stops: [.init(color: .clear, location: 0.42), .init(color: bg.opacity(0.82), location: 0.8), .init(color: bg, location: 1)],
                startPoint: .top, endPoint: .bottom)
        }
        .allowsHitTesting(false)
    }

    private func details(_ hero: Hero) -> some View {
        let tile = hero.tile
        return VStack(alignment: .leading, spacing: metrics.wide ? 14 : 10) {
            TitleLogo(logo: tile.logo, name: tile.name, maxWidth: metrics.wide ? 440 : 260, height: metrics.wide ? 120 : 88)
            Text(hero.reason)
                .font(.display(metrics.wide ? Theme.type.title1 : Theme.type.title2, italic: true))
                .foregroundStyle(Theme.Palette.text)
            if let meta = meta(tile) {
                Text(meta)
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
            }
            HStack(spacing: 12) {
                NavigationLink(value: TitleRoute(tile)) {
                    Label("Play", systemImage: "play.fill")
                }
                .buttonStyle(PillButtonStyle(prominent: true))
                NavigationLink(value: TitleRoute(tile)) {
                    Label("Details", systemImage: "info.circle")
                }
                .buttonStyle(PillButtonStyle())
            }
            .padding(.top, 6)
        }
        .frame(maxWidth: metrics.wide ? 620 : .infinity, alignment: .leading)
    }

    private func meta(_ tile: Tile) -> String? {
        let parts = [tile.releaseInfo, kindLabel(tile.kind)].compactMap { $0 }
        return parts.isEmpty ? nil : parts.joined(separator: "  ·  ")
    }
}

func kindLabel(_ kind: String) -> String? {
    switch kind {
    case "movie": "Movie"
    case "series": "Series"
    case "channel": "Channel"
    case "tv": "Live TV"
    default: nil
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
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        ZStack(alignment: .bottomLeading) {
            switch loaded {
            case .some(.some(let art)):
                Image(decorative: art.image, scale: 1)
                    .resizable()
                    .scaledToFit()
                    // Logos keep their colours; on a light page a soft shadow keeps
                    // white letters from vanishing into the cream.
                    .shadow(color: .black.opacity(scheme == .light ? 0.35 : 0), radius: 6, y: 2)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomLeading)
                    .transition(.opacity)
            case .some(.none):
                Text(name)
                    .font(.display(nameSize))
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
