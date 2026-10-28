import SwiftUI

/// "More like this": Trakt's related titles as 2:3 posters, the tiles Home's rows show, with no source name.
/// Hidden while empty (it lands late, under what it follows, and moves nothing above it).
struct SimilarRow: View {
    let tiles: [Tile]
    @Environment(\.metrics) private var metrics

    var body: some View {
        if !tiles.isEmpty {
            VStack(alignment: .leading, spacing: metrics.wide ? 16 : 12) {
                RowHeader(title: "More like this").padding(.horizontal, metrics.wide ? metrics.gutter : 20)
                Shelf(spacing: metrics.wide ? Theme.TileDesktop.gap : Theme.Tile.gap, inset: metrics.wide ? metrics.gutter : 20, room: 12) {
                    ForEach(tiles.keyed, id: \.key) { _, tile in
                        NavigationLink(value: TitleRoute(tile)) { PosterTile(tile: tile) }
                            .buttonStyle(CardButtonStyle())
                    }
                }
            }
            #if os(tvOS)
            .focusSection()
            #endif
        }
    }
}
