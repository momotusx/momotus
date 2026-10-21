import SwiftUI

/// One Home row: a titled strip of posters, or an explained state.
struct RowView: View {
    let row: HomeRow
    let retry: () -> Void
    /// TV: this row is Home's start; its first tile is the scope's default focus.
    var startScope: Namespace.ID?
    @Environment(\.metrics) private var metrics
    @Environment(CoreClient.self) private var core
    /// Remove from Continue Watching did not go through: the title and why.
    @State private var removeFailure: (tile: Tile, words: String)?

    var body: some View {
        VStack(alignment: .leading, spacing: metrics.wide ? 16 : 12) {
            RowHeader(title: row.title, subtitle: subtitle).padding(.horizontal, metrics.gutter)
            switch row.state {
            case .loading, .slow:
                SkeletonStrip()
            case .empty:
                StateLine(symbol: "inbox", text: "\(row.source) has nothing in this catalog right now.")
                    .padding(.horizontal, metrics.gutter)
            case .failed(let reason):
                // Secondary: the page's primary is the hero's Play, or the one Try again when nothing answered.
                StateLine(symbol: "triangle-alert", text: "\(row.source) did not answer: \(reason)", action: ("Try again", retry), primary: false)
                    .padding(.horizontal, metrics.gutter)
            case .ready:
                strip
                if let removeFailure {
                    StateLine(symbol: "triangle-alert", text: removeFailure.words, action: ("Try again", { remove(removeFailure.tile) }), primary: false)
                        .padding(.horizontal, metrics.gutter)
                }
            }
        }
        #if os(tvOS)
        .focusSection()
        #endif
    }

    private var subtitle: String? {
        if row.state == .slow { return "\(row.source), still loading" }
        return row.rowId == "continue-watching" ? nil : row.source
    }

    /// Takes the title off Continue Watching; a failure says so under the row,
    /// with Try again, as a failed row does.
    private func remove(_ tile: Tile) {
        removeFailure = nil
        Task {
            do {
                try await core.perform("removeFromContinue", ["itemId": tile.id])
            } catch {
                coreLog.error("remove from continue watching: \(error.localizedDescription, privacy: .public)")
                removeFailure = (tile, removeFailedWords(name: tile.name, reason: error.localizedDescription))
            }
        }
    }

    private var strip: some View {
        ScrollView(.horizontal) {
            LazyHStack(alignment: .top, spacing: metrics.wide ? 20 : 12) {
                let tiles = row.tiles.keyed
                ForEach(tiles, id: \.key) { key, tile in
                    NavigationLink(value: TitleRoute(tile)) {
                        if row.rowId == "continue-watching" {
                            ContinueCard(tile: tile)
                        } else {
                            PosterTile(tile: tile)
                        }
                    }
                    .buttonStyle(CardButtonStyle())
                    // Continue Watching and Next Up: a title can leave the row (right click, long press,
                    // a long press of select on TV) until it is played again, on every synced device.
                    .contextMenu {
                        if row.rowId == "continue-watching" {
                            Button("Remove from Continue Watching", glyph: "x") { remove(tile) }
                        }
                    }
                    // TV: Home's start row enters on its first tile.
                    .modifier(StartFocus(scope: key == tiles.first?.key ? startScope : nil))
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

/// A section header: a bar, the title, its source and a hairline to the edge.
struct RowHeader: View {
    let title: String
    var subtitle: String?
    @Environment(\.metrics) private var metrics

    var body: some View {
        HStack(spacing: metrics.wide ? 14 : 8) {
            Capsule()
                .fill(Theme.Palette.text)
                .frame(width: metrics.wide ? 4 : 3, height: size * 1.05)
            Text(title)
                .display(size)
                .foregroundStyle(Theme.Palette.text)
                .lineLimit(2)
                .layoutPriority(1)
                .accessibilityAddTraits(.isHeader)
            if let subtitle {
                Text(subtitle)
                    .font(.text(metrics.cardInfo + 1, weight: .medium, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.dim)
                    .lineLimit(1)
            }
            Rectangle()
                .fill(Theme.Palette.lineStrong)
                .frame(height: 1)
                .frame(minWidth: 12, maxWidth: .infinity)
                .padding(.leading, 4)
                .accessibilityHidden(true)
        }
    }

    private var size: CGFloat {
        #if os(tvOS)
        return Theme.type.title3
        #else
        return metrics.wide ? 24 : 19
        #endif
    }
}

/// A catalog poster: the art with its rating chip, the name and the year
/// (or the core's caption) under it, never over it.
struct PosterTile: View {
    let tile: Tile
    @Environment(\.metrics) private var metrics

    var body: some View {
        VStack(alignment: .leading, spacing: metrics.wide ? 10 : 6) {
            ArtImage(url: tile.poster, fallback: tile.name)
                .frame(width: size.width, height: size.height)
                .overlay(alignment: .topTrailing) {
                    if !ratingParts(tile.ratings).isEmpty { RatingChip(ratings: ratingParts(tile.ratings)).padding(metrics.chip * 0.55) }
                }
                .cardArt()
            CardCaption(name: tile.name, info: tile.releaseInfo).frame(width: size.width, alignment: .leading)
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel(([tile.name, tile.releaseInfo].compactMap { $0 } + ratingParts(tile.ratings).map(spokenScore)).joined(separator: ", "))
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

/// A row before Home has any: the header's bar and a blank title block over
/// placeholder posters. No words, so it never stands in for real content.
struct SkeletonRow: View {
    @Environment(\.metrics) private var metrics

    var body: some View {
        VStack(alignment: .leading, spacing: metrics.wide ? 16 : 12) {
            HStack(spacing: metrics.wide ? 14 : 8) {
                Capsule().fill(Theme.Palette.lineStrong).frame(width: metrics.wide ? 4 : 3, height: metrics.wide ? 25 : 20)
                RoundedRectangle(cornerRadius: 6, style: .continuous)
                    .fill(Theme.Palette.surface)
                    .frame(width: metrics.wide ? 220 : 150, height: metrics.wide ? 20 : 16)
            }
            .padding(.horizontal, metrics.gutter)
            SkeletonStrip()
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Loading")
    }
}

/// Placeholder posters while a catalog loads, breathing slowly.
private struct SkeletonStrip: View {
    @Environment(\.metrics) private var metrics
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var dim = false

    var body: some View {
        // The posters hang in an overlay so their row never widens the page:
        // as a plain child it pushed a whole Search page off the side.
        Color.clear
            .frame(height: metrics.poster.height)
            .overlay(alignment: .leading) {
                HStack(spacing: metrics.wide ? 20 : 12) {
                    ForEach(0..<10, id: \.self) { _ in
                        RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous)
                            .fill(Theme.Palette.surface)
                            .frame(width: metrics.poster.width, height: metrics.poster.height)
                    }
                }
                .padding(.horizontal, metrics.gutter)
            }
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
