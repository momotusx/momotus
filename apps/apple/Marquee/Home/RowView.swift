import SwiftUI

/// One Home row: a titled strip of posters, or an explained state.
struct RowView: View {
    let row: HomeRow
    let retry: () -> Void
    /// Called when a tile is opened (Search remembers the query then).
    var onOpen: (() -> Void)?
    @Environment(\.metrics) private var metrics

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            header.padding(.horizontal, metrics.gutter)
            switch row.state {
            case .loading, .slow:
                SkeletonStrip()
            case .empty:
                StateLine(symbol: "tray", text: "Nothing in this catalog right now.")
                    .padding(.horizontal, metrics.gutter)
            case .failed(let reason):
                StateLine(symbol: "exclamationmark.triangle", text: "\(row.source) did not answer: \(reason)", action: ("Try again", retry))
                    .padding(.horizontal, metrics.gutter)
            case .ready:
                strip
            }
        }
        #if os(tvOS)
        .focusSection()
        #endif
    }

    private var header: some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Text(row.title)
                .font(.display(Theme.type.title2))
                .foregroundStyle(Theme.Palette.text)
                .accessibilityAddTraits(.isHeader)
            if let subtitle {
                Text(subtitle)
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
            }
        }
    }

    private var subtitle: String? {
        if row.state == .slow { return "\(row.source), still loading" }
        return row.rowId == "continue-watching" ? nil : row.source
    }

    private var strip: some View {
        ScrollView(.horizontal) {
            LazyHStack(alignment: .top, spacing: metrics.wide ? 20 : 12) {
                ForEach(Array(row.tiles.enumerated()), id: \.offset) { _, tile in
                    NavigationLink(value: TitleRoute(tile)) {
                        if row.rowId == "continue-watching" {
                            ContinueCard(tile: tile)
                        } else {
                            PosterTile(tile: tile)
                        }
                    }
                    .buttonStyle(CardButtonStyle())
                    .accessibilityLabel(tile.name)
                    .simultaneousGesture(TapGesture().onEnded { onOpen?() })
                }
            }
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, 12)
        }
        .scrollIndicators(.hidden)
        .scrollClipDisabled()
        .padding(.vertical, -12)
    }
}

/// Poster art for a catalog tile.
struct PosterTile: View {
    let tile: Tile
    @Environment(\.metrics) private var metrics

    var body: some View {
        ArtImage(url: tile.poster, fallback: tile.name)
            .frame(width: size.width, height: size.height)
            .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous))
            .accessibilityElement(children: .combine)
            .accessibilityLabel(tile.name)
    }

    private var size: CGSize {
        let height = metrics.poster.height
        switch tile.posterShape {
        case "landscape": return CGSize(width: height * 16 / 9, height: height)
        case "square": return CGSize(width: height, height: height)
        default: return metrics.poster
        }
    }
}

/// "S2:E3" from a Stremio video id like "tt0903747:2:3".
func episodeLabel(_ videoId: String?) -> String? {
    guard let parts = videoId?.split(separator: ":"), parts.count == 3 else { return nil }
    return "S\(parts[1]):E\(parts[2])"
}

/// Placeholder posters while a catalog loads, breathing slowly.
private struct SkeletonStrip: View {
    @Environment(\.metrics) private var metrics
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var dim = false

    var body: some View {
        HStack(spacing: metrics.wide ? 20 : 12) {
            ForEach(0..<10, id: \.self) { _ in
                RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous)
                    .fill(Theme.Palette.surface)
                    .frame(width: metrics.poster.width, height: metrics.poster.height)
            }
        }
        .padding(.horizontal, metrics.gutter)
        .frame(maxWidth: .infinity, alignment: .leading)
        .clipped()
        .opacity(dim ? 0.55 : 1)
        .animation(reduceMotion ? nil : .easeInOut(duration: 1.1).repeatForever(), value: dim)
        .onAppear { dim = true }
        .accessibilityLabel("Loading")
    }
}

/// Where a tile leads: the title page for one id.
struct TitleRoute: Hashable {
    let kind: String
    let id: String
    let name: String
    /// The season to show first (from the calendar).
    var season: Int?

    init(kind: String, id: String, name: String, season: Int? = nil) {
        (self.kind, self.id, self.name, self.season) = (kind, id, name, season)
    }

    init(_ tile: Tile) {
        self.init(kind: tile.kind, id: tile.id, name: tile.name)
    }
}
