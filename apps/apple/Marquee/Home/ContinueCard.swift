import SwiftUI

/// A Continue Watching or Next Up card: a 16:9 still (the episode still,
/// else the backdrop, else the poster cropped from its upper part) with
/// Apple's white progress line, then the name and the core's caption under it.
struct ContinueCard: View {
    let tile: Tile
    @Environment(\.metrics) private var metrics

    var body: some View {
        VStack(alignment: .leading, spacing: metrics.wide ? 10 : 6) {
            ArtImage(url: tile.background ?? tile.poster, maxPixels: ArtSize.backdrop / 2, fallback: tile.name, alignment: tile.background == nil ? .top : .center)
                .overlay(alignment: .bottom) {
                    if let progress = tile.progress { EdgeProgress(value: progress) }
                }
                .frame(width: size.width, height: size.height)
                .cardArt(radius: radius)
            CardCaption(name: tile.name, info: caption).frame(width: size.width)
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(tile.name), \(spokenCodes(caption))")
    }

    /// 16:9 lockups: TV's 410, tv.apple.com's 256 on the Mac, the iPhone's 239.
    private var size: CGSize {
        let width: CGFloat = metrics.wide ? (metrics.poster.width >= Theme.Poster.tv.width ? Theme.Poster.landscape.width : Theme.TileDesktop.landscape.width) : Theme.Tile.landscape.width
        return CGSize(width: width, height: width * 9 / 16)
    }

    /// Apple's corners: 14 on the web and the Mac, 13 on the iPhone, the token on TV.
    private var radius: CGFloat {
        #if os(tvOS)
        return Theme.Radius.m
        #else
        return metrics.wide ? 14 : 13
        #endif
    }

    /// The core's one caption, as is: "S2:E3 · 36 min left", "36 min left", "Next: S1:E2".
    private var caption: String { tile.releaseInfo ?? "" }
}

/// Progress at the foot of card art: the white line on its dark track, over
/// a dark strip so it reads on white art too. 4 pt on every form.
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
                    Capsule().fill(ProgressLine.track)
                    Capsule().fill(ProgressLine.fill).frame(width: geo.size.width * min(max(value, 0), 1))
                }
            }
            .frame(height: 4)
            .padding(.horizontal, inset)
            .padding(.bottom, inset)
        }
        .accessibilityHidden(true)
    }
}
