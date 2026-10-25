import SwiftUI

/// A catalog tile: always a poster, whatever shape its addon draws the catalog in, as Stremio shows them (the
/// owner, 2026-10-23); only Continue Watching keeps 16:9 stills (`ContinueCard`). The cover, for an IMDb title,
/// is RPDB's with the ratings band drawn in (the addon's poster when RPDB has none). The iPhone's and
/// tv.apple.com's shelves show the art alone; VoiceOver still says the name, the year and the score. tvOS keeps
/// its v2 tile (D9): the name and the year under the art, no score.
struct PosterTile: View {
    let tile: Tile
    @Environment(\.metrics) private var metrics

    var body: some View {
        content
            .accessibilityElement(children: .combine)
            .accessibilityLabel(posterLabel([tile.rank.map { "Number \($0)" }, tile.name, tile.genre ?? tile.releaseInfo], tile.ratings))
    }

    @ViewBuilder private var content: some View {
        let art = ArtImage(url: tile.poster, fallbackURL: tile.posterFallback, fallback: tile.name)
            .frame(width: metrics.poster.width, height: metrics.poster.height)
        #if os(tvOS)
        VStack(alignment: .leading, spacing: metrics.wide ? 10 : 6) {
            art
                .cardArt(radius: metrics.posterRadius)
            CardCaption(name: tile.name, info: tile.releaseInfo).frame(width: metrics.poster.width, alignment: .leading)
        }
        #else
        // A Top 10 tile: its place over the art's top left, its genre one line under the art (the owner's group C,
        // C2 until picked; plans/appletv-polish.md 4.3).
        VStack(alignment: .leading, spacing: 6) {
            art.cardArt(radius: metrics.posterRadius)
                .overlay(alignment: .topLeading) {
                    if let rank = tile.rank { RankNumeral(rank: rank, size: metrics.wide ? 40 : 27) }
                }
            if let genre = tile.genre {
                Text(genre)
                    .textFont(metrics.wide ? 13 : 11, relativeTo: .caption)
                    .foregroundStyle(Theme.Palette.dim)
                    .lineLimit(1)
                    .frame(width: metrics.poster.width, alignment: .leading)
            }
        }
        #endif
    }
}

/// A Top 10 tile's place: white, bold, over a soft shade in the art's corner.
private struct RankNumeral: View {
    let rank: Int
    let size: CGFloat

    var body: some View {
        Text("\(rank)")
            .display(size)
            .foregroundStyle(.white)
            .shadow(color: .black.opacity(0.55), radius: 4, y: 1)
            .padding(.leading, size / 3)
            .padding(.top, size / 8)
            .background(alignment: .topLeading) {
                RadialGradient(colors: [.black.opacity(0.45), .clear], center: .topLeading, startRadius: 0, endRadius: size * 2.6)
                    .frame(width: size * 3, height: size * 3)
                    .allowsHitTesting(false)
            }
            .accessibilityHidden(true)
    }
}
