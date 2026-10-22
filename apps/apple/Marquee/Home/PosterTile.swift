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
            .accessibilityLabel(posterLabel([tile.name, tile.releaseInfo], tile.ratings))
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
        art.cardArt(radius: metrics.posterRadius)
        #endif
    }
}
