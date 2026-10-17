import SwiftUI

/// The home hero: an inset rounded card holding the backdrop, why the title
/// is here, its logo (or its name), the meta line, three lines of synopsis,
/// Play and Details. Holds its height while empty so rows never jump.
struct HeroView: View {
    let hero: Hero?
    let height: CGFloat
    @Environment(\.metrics) private var metrics

    var body: some View {
        let card = RoundedRectangle(cornerRadius: Theme.Radius.hero, style: .continuous)
        ZStack(alignment: .bottomLeading) {
            ArtImage(url: hero?.tile.background ?? hero?.tile.poster, maxPixels: ArtSize.backdrop)
                .accessibilityHidden(true)
            scrims
            if let hero {
                details(hero)
                    .padding(.leading, metrics.wide ? Theme.Space.xl - 12 : 18)
                    .padding(.trailing, metrics.wide ? Theme.Space.l : 18)
                    .padding(.bottom, metrics.wide ? Theme.Space.l : 20)
            }
        }
        .frame(maxWidth: .infinity)
        .frame(height: height)
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
                LinearGradient(
                    stops: [
                        .init(color: bg.opacity(0.94), location: 0), .init(color: bg.opacity(0.72), location: 0.28),
                        .init(color: bg.opacity(0.18), location: 0.58), .init(color: .clear, location: 0.72),
                    ],
                    startPoint: .leading, endPoint: .trailing)
                LinearGradient(
                    stops: [.init(color: bg.opacity(0.9), location: 0), .init(color: bg.opacity(0.3), location: 0.34), .init(color: .clear, location: 0.58)],
                    startPoint: .bottom, endPoint: .top)
            } else {
                // A 72 pt band under the wordmark, then a fade: the white mark
                // reads on bright art too.
                LinearGradient(
                    stops: [
                        .init(color: bg.opacity(0.78), location: 0), .init(color: bg.opacity(0.6), location: 72 / height),
                        .init(color: .clear, location: 140 / height),
                    ],
                    startPoint: .top, endPoint: .bottom)
                LinearGradient(
                    stops: [.init(color: bg.opacity(0.96), location: 0), .init(color: bg.opacity(0.55), location: 0.38), .init(color: .clear, location: 0.62)],
                    startPoint: .bottom, endPoint: .top)
            }
        }
        .allowsHitTesting(false)
    }

    private func details(_ hero: Hero) -> some View {
        let tile = hero.tile
        let wide = metrics.wide
        return VStack(alignment: .leading, spacing: 0) {
            Eyebrow(text: hero.reason)
                .padding(.bottom, wide ? 14 : 10)
            TitleLogo(
                logo: tile.logo, name: tile.name, maxWidth: wide ? 440 : 260, height: wide ? 120 : 84,
                nameSize: wide ? Theme.type.hero + 12 : Theme.type.title1)
                .padding(.bottom, wide ? 18 : 12)
            MetaLine(rating: tile.rating, parts: heroMeta(hero), size: wide ? Theme.type.callout : 13)
                .foregroundStyle(Theme.Palette.muted)
                .padding(.bottom, wide ? 14 : 10)
            if let description = hero.description {
                Text(description)
                    .font(.text(wide ? Theme.type.callout : 14, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
                    .lineLimit(3)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.bottom, wide ? 22 : 16)
            }
            HStack(alignment: .top, spacing: wide ? 24 : 14) {
                NavigationLink(value: TitleRoute(tile)) {
                    Label("Play", glyph: "play-fill")
                }
                .buttonStyle(PillButtonStyle(prominent: true))
                NavigationLink(value: TitleRoute(tile)) {
                    Label("Details", glyph: "info")
                }
                .buttonStyle(RoundButtonStyle())
            }
        }
        .frame(maxWidth: wide ? 620 : .infinity, alignment: .leading)
    }
}

/// The hero's meta after the rating: year, runtime and genres, as the core gives them.
func heroMeta(_ hero: Hero) -> [String] {
    let genres = (hero.genres ?? []).joined(separator: ", ")
    return [hero.tile.releaseInfo, hero.runtime, genres.isEmpty ? nil : genres].compactMap { $0 }
}

/// An eyebrow over artwork: accent spaced capitals in a chip filled with the
/// page colour. Accent text holds 4.5:1 on `bg` only; over light art it
/// falls to 3.7:1 even under the hero's scrim (DECISION.md, Accent).
struct Eyebrow: View {
    let text: String
    @Environment(\.metrics) private var metrics

    var body: some View {
        Text(text.uppercased())
            .font(.text(Theme.type.caption, weight: .bold, relativeTo: .caption))
            .tracking(Theme.type.caption * 0.14)
            .foregroundStyle(Theme.Palette.accent)
            .lineLimit(2)
            .padding(.horizontal, padding.width)
            .padding(.vertical, padding.height)
            .background(Capsule().fill(Theme.Palette.bg))
    }

    /// The web's chip: 8 by 14 on the TV canvas, 5 by 10 on a phone.
    private var padding: CGSize {
        #if os(tvOS)
        return CGSize(width: 14, height: 8)
        #else
        return metrics.wide ? CGSize(width: 11, height: 6) : CGSize(width: 10, height: 5)
        #endif
    }
}

/// "★ 9.2 · 2011–2019 · Drama, Fantasy · 8 seasons": the gold star beside the
/// rating, the rest joined by dots. Lines break only between the parts.
struct MetaLine: View {
    let rating: String?
    let parts: [String]
    let size: CGFloat

    var body: some View {
        FlowRow(spacing: size * 0.45) {
            if let rating {
                HStack(spacing: size * 0.3) {
                    Glyph("star-fill", size: size * 0.9, relativeTo: .callout).foregroundStyle(Theme.Palette.star)
                    Text(rating).fontWeight(.bold).foregroundStyle(Theme.Palette.text)
                }
            }
            ForEach(Array(parts.enumerated()), id: \.offset) { index, part in
                if rating != nil || index > 0 {
                    Text("\u{B7}").foregroundStyle(Theme.Palette.dim)
                }
                Text(part)
            }
        }
        .font(.text(size, weight: .semibold, relativeTo: .callout))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(([rating.map { "Rated \($0)" }] + parts).compactMap { $0 }.joined(separator: ", "))
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
