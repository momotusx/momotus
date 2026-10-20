import SwiftUI

/// A Continue Watching or Next Up card: a 16:9 still (the episode still,
/// else the backdrop, else the poster) with a thin accent progress line,
/// then the name and the core's caption under it.
struct ContinueCard: View {
    let tile: Tile
    @Environment(\.metrics) private var metrics

    var body: some View {
        VStack(alignment: .leading, spacing: metrics.wide ? 10 : 6) {
            ArtImage(url: tile.background ?? tile.poster, maxPixels: ArtSize.backdrop / 2, fallback: tile.name)
                .overlay(alignment: .bottom) {
                    if let progress = tile.progress { EdgeProgress(value: progress) }
                }
                .frame(width: size.width, height: size.height)
                .cardArt()
            CardCaption(name: tile.name, info: caption).frame(width: size.width)
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(tile.name), \(spokenCodes(caption))")
    }

    private var size: CGSize {
        let width: CGFloat = metrics.wide ? (metrics.poster.width >= Theme.Poster.tv.width ? Theme.Poster.landscape.width : 320) : 240
        return CGSize(width: width, height: width * 9 / 16)
    }

    /// The core's one caption, as is: "S2:E3 · 36 min left", "36 min left", "Next: S1:E2".
    private var caption: String { tile.releaseInfo ?? "" }
}

/// Progress at the foot of card art: a thin accent line on its track, over
/// a dark strip so it reads on white art too.
struct EdgeProgress: View {
    let value: Double
    @Environment(\.metrics) private var metrics

    var body: some View {
        let inset: CGFloat = metrics.wide ? 12 : 6
        ZStack(alignment: .bottom) {
            LinearGradient(colors: [.clear, .black.opacity(0.65)], startPoint: .top, endPoint: .bottom)
                .frame(height: metrics.wide ? 30 : 20)
            GeometryReader { geo in
                ZStack(alignment: .leading) {
                    Capsule().fill(.white.opacity(0.28))
                    Capsule().fill(Theme.Palette.accent).frame(width: geo.size.width * min(max(value, 0), 1))
                }
            }
            .frame(height: metrics.wide ? 4 : 3)
            .padding(.horizontal, inset)
            .padding(.bottom, inset)
        }
        .accessibilityHidden(true)
    }
}
