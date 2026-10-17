import SwiftUI

/// A Continue Watching or Next Up card: 16:9 art (the episode still, else the
/// backdrop, else the poster), the logo or title bottom left, the episode
/// below, and progress on the bottom edge.
struct ContinueCard: View {
    let tile: Tile
    @Environment(\.metrics) private var metrics

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            ArtImage(url: tile.background ?? tile.poster, maxPixels: ArtSize.backdrop / 2)
                .overlay {
                    LinearGradient(
                        stops: [.init(color: .clear, location: 0.45), .init(color: .black.opacity(0.75), location: 1)],
                        startPoint: .top, endPoint: .bottom)
                }
                .overlay(alignment: .bottomLeading) {
                    TitleLogo(logo: tile.logo, name: tile.name, maxWidth: size.width * 0.55, height: size.height * 0.26, nameSize: Theme.type.title3)
                        .padding(.horizontal, 12)
                        .padding(.bottom, tile.progress == nil ? 14 : 24)
                }
                .overlay(alignment: .bottom) {
                    if let progress = tile.progress { EdgeProgress(value: progress) }
                }
                .frame(width: size.width, height: size.height)
                .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous))
            Text(caption)
                .font(.text(Theme.type.callout, relativeTo: .callout))
                .foregroundStyle(Theme.Palette.muted)
                .lineLimit(1)
                .frame(width: size.width, alignment: .leading)
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(tile.name), \(caption)")
    }

    private var size: CGSize {
        let width: CGFloat = metrics.wide ? (metrics.poster.width >= Theme.Poster.tv.width ? Theme.Poster.landscape.width : 320) : 260
        return CGSize(width: width, height: width * 9 / 16)
    }

    /// The core's one caption, as is: "S2:E3 · 36 min left", "36 min left", "Next: S1:E2".
    private var caption: String { tile.releaseInfo ?? "" }
}

/// Progress inside a card or poster bottom strip: a 4 pt bar, track and fill, on a
/// 32 pt dark scrim so it reads on white art too.
struct EdgeProgress: View {
    let value: Double

    var body: some View {
        ZStack(alignment: .bottom) {
            LinearGradient(colors: [.clear, .black.opacity(0.6)], startPoint: .top, endPoint: .bottom)
                .frame(height: 32)
            GeometryReader { geo in
                ZStack(alignment: .leading) {
                    Capsule().fill(.white.opacity(0.24))
                    Capsule().fill(.white).frame(width: geo.size.width * min(max(value, 0), 1))
                }
            }
            .frame(height: 4)
            .padding(.horizontal, 12)
            .padding(.bottom, 10)
        }
        .accessibilityHidden(true)
    }
}
