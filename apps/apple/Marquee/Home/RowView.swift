import SwiftUI

/// One Home row: a titled strip of posters, or an explained state.
struct RowView: View {
    let row: HomeRow
    let retry: () -> Void
    /// TV: this row is Home's start; its first tile is the scope's default focus.
    var startScope: Namespace.ID?
    @Environment(\.metrics) private var metrics
    /// Home's Remove from Continue Watching, with its Undo; nil where a row cannot lose a title (Search).
    var removal: ContinueRemoval?

    var body: some View {
        VStack(alignment: .leading, spacing: metrics.wide ? 16 : 12) {
            RowHeader(title: row.title, subtitle: row.subtitle).padding(.horizontal, metrics.gutter)
            switch row.state {
            case .loading, .slow:
                SkeletonStrip()
            case .empty:
                StateLine(symbol: "inbox", text: row.stateWords ?? "")
                    .padding(.horizontal, metrics.gutter)
            case .failed:
                // Secondary: the page's primary is the hero's Play, or the one Try again when nothing answered.
                StateLine(symbol: "triangle-alert", text: row.stateWords ?? "", action: ("Try again", retry), primary: false)
                    .padding(.horizontal, metrics.gutter)
            case .ready:
                strip
                if let removal, continues { RemovalLine(removal: removal).padding(.horizontal, metrics.gutter) }
            }
        }
        #if os(tvOS)
        .focusSection()
        #endif
    }

    private var strip: some View {
        Shelf(spacing: metrics.wide ? Theme.TileDesktop.gap : Theme.Tile.gap, inset: metrics.gutter, room: 12) {
            // A title waiting for its Undo is out of the row.
            let tiles = (continues ? row.tiles.filter { removal?.hides($0) != true } : row.tiles).keyed
            ForEach(tiles, id: \.key) { key, tile in
                NavigationLink(value: TitleRoute(tile)) {
                    if continues {
                        ContinueCard(tile: tile)
                    } else {
                        PosterTile(tile: tile)
                    }
                }
                .buttonStyle(CardButtonStyle())
                // Continue Watching and Next Up: a title can leave the row (right click, long press,
                // a long press of select on TV) until it is played again, on every synced device.
                .contextMenu {
                    if let removal, continues {
                        Button("Remove from Continue Watching", glyph: "x") { removal.remove(tile) }
                    }
                }
                #if os(iOS)
                // Touch has no right click and a long press is hidden: the X on the card, as the web's phone shows it.
                .overlay(alignment: .topTrailing) {
                    if let removal, continues { RemoveTileButton(name: tile.name) { removal.remove(tile) } }
                }
                #endif
                // TV: Home's start row enters on its first tile.
                .modifier(StartFocus(scope: key == tiles.first?.key ? startScope : nil))
            }
        }
    }

    private var continues: Bool { row.rowId == SettingsRules.continueRow }
}

/// Under Continue Watching after Remove: the words with Undo while it can be taken back, or why it failed with
/// Try again, as a failed row says it.
private struct RemovalLine: View {
    let removal: ContinueRemoval

    var body: some View {
        if let tile = removal.removing {
            StateLine(symbol: "check", text: removedWords(name: tile.name), action: ("Undo", removal.undo), spoken: true)
        } else if let failed = removal.failed {
            StateLine(symbol: "triangle-alert", text: failed.words, action: ("Try again", { removal.remove(failed.tile) }), spoken: true)
        }
    }
}

#if os(iOS)
/// The X on a Continue Watching card, as the web's phone draws it (`.tile-x`): a dark disc over the still at its
/// top corner, 44 pt to touch.
private struct RemoveTileButton: View {
    let name: String
    let run: () -> Void

    var body: some View {
        Button(action: run) {
            Glyph("x", size: 16)
                .foregroundStyle(.white)
                .frame(width: 30, height: 30)
                // The web's disc (`rgba(0, 0, 0, .62)`): white reads on it over white art.
                .background(.black.opacity(0.62), in: Circle())
                .frame(width: Theme.minTarget, height: Theme.minTarget)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Remove \(name) from Continue Watching")
    }
}
#endif

/// A section header: the title, and a quiet word beside it ("Slow to answer, still trying").
struct RowHeader: View {
    let title: String
    var subtitle: String?
    @Environment(\.metrics) private var metrics

    var body: some View {
        // The shelf title in the `title3` role, as Apple's shelves: no bar, no hairline, no All pill.
        HStack(alignment: .firstTextBaseline, spacing: metrics.wide ? 14 : 8) {
            Text(title)
                .display(size)
                .foregroundStyle(Theme.Palette.text)
                .lineLimit(2)
                .layoutPriority(1)
                .accessibilityAddTraits(.isHeader)
            if let subtitle {
                Text(subtitle)
                    .textFont(metrics.cardInfo + 1, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.dim)
                    .lineLimit(1)
            }
            Spacer(minLength: 0)
        }
    }

    /// `title3`: 38 on TV, 20 on a phone, and tv.apple.com's 17 on a wide screen.
    private var size: CGFloat {
        #if os(tvOS)
        return Theme.type.title3
        #else
        return metrics.wide ? 17 : Theme.type.title3
        #endif
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
            // The shelf title's place, then the posters' shapes.
            RoundedRectangle(cornerRadius: 6, style: .continuous)
                .fill(Theme.Palette.surface)
                .frame(width: metrics.wide ? 220 : 150, height: metrics.wide ? 20 : 16)
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
                HStack(spacing: metrics.wide ? Theme.TileDesktop.gap : Theme.Tile.gap) {
                    ForEach(0..<10, id: \.self) { _ in
                        RoundedRectangle(cornerRadius: metrics.posterRadius, style: .continuous)
                            .fill(Theme.Palette.surface)
                            .frame(width: metrics.poster.width, height: metrics.poster.height)
                    }
                }
                .padding(.horizontal, metrics.gutter)
            }
            .clipped()
        // Scoped to the opacity: an endless animation on the view itself also ran its removal, so
        // Home's loading rows stayed drawn under the real ones.
        .animation(reduceMotion ? nil : .easeInOut(duration: 1.1).repeatForever()) { $0.opacity(dim ? 0.55 : 1) }
        .onAppear { dim = true }
        .accessibilityLabel("Loading")
    }
}

