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
                        .padding(.bottom, 14)
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

    /// "Next: S1 E2" for Next Up, "S2:E3, 37% watched", or "Movie, 25% watched".
    private var caption: String {
        if let next = tile.releaseInfo, next.hasPrefix("Next") { return next }
        let what = episodeLabel(tile.videoId) ?? kindLabel(tile.kind) ?? ""
        guard let progress = tile.progress else { return what }
        return "\(what), \(Int((progress * 100).rounded()))% watched"
    }
}

/// Progress on a card's bottom edge: a dark track so it reads on any image.
private struct EdgeProgress: View {
    let value: Double

    var body: some View {
        GeometryReader { geo in
            ZStack(alignment: .leading) {
                Rectangle().fill(.black.opacity(0.55))
                Rectangle().fill(Color.white).frame(width: geo.size.width * min(max(value, 0), 1))
            }
        }
        .frame(height: 4)
        .accessibilityHidden(true)
    }
}
