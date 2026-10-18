import SwiftUI

/// One choice in a chip row.
struct Chip<ID: Hashable>: Identifiable {
    let id: ID
    let title: String
    var detail: String?
}

/// "TYPE  Movies  Series": a small caps label and its chips. Wraps on wide
/// screens, scrolls sideways on phones.
struct ChipRow<ID: Hashable>: View {
    let label: String
    let chips: [Chip<ID>]
    let isSelected: (ID) -> Bool
    let select: (ID) -> Void
    @Environment(\.metrics) private var metrics

    var body: some View {
        if metrics.wide {
            HStack(alignment: .firstTextBaseline, spacing: Theme.Space.s) {
                caption.frame(width: 104, alignment: .leading)
                FlowRow(spacing: 10) { buttons }
            }
        } else {
            VStack(alignment: .leading, spacing: 8) {
                caption
                ScrollView(.horizontal) {
                    HStack(spacing: 8) { buttons }.padding(.vertical, 4)
                }
                .scrollIndicators(.hidden)
                .scrollClipDisabled()
                // A chip cut at the edge fades out over 24 pt, so it reads as "more this way".
                .mask {
                    HStack(spacing: 0) {
                        Color.black
                        LinearGradient(colors: [.black, .clear], startPoint: .leading, endPoint: .trailing).frame(width: 24)
                    }
                    .padding(.leading, -metrics.gutter)
                    .padding(.vertical, -8)
                }
            }
        }
    }

    private var caption: some View {
        Text(label.uppercased())
            .font(.text(Theme.type.caption, weight: .semibold, relativeTo: .caption))
            .tracking(1.6)
            .foregroundStyle(Theme.Palette.muted)
    }

    @ViewBuilder private var buttons: some View {
        ForEach(chips) { chip in
            Button { select(chip.id) } label: {
                HStack(spacing: 6) {
                    // Selection is a fill and a check, never a ring: the ring means focus.
                    if isSelected(chip.id) { Glyph("check", size: Theme.type.callout, relativeTo: .callout) }
                    Text(chip.title)
                    if let detail = chip.detail {
                        Text(detail).fontWeight(.regular).opacity(0.7)
                    }
                }
            }
            .buttonStyle(ChipStyle(selected: isSelected(chip.id)))
            .accessibilityAddTraits(isSelected(chip.id) ? .isSelected : [])
        }
    }
}

/// Selected chips are white with dark words; the rest are a faint white fill.
struct ChipStyle: ButtonStyle {
    let selected: Bool

    func makeBody(configuration: Configuration) -> some View {
        ChipBody(configuration: configuration, selected: selected)
    }

    private struct ChipBody: View {
        let configuration: ButtonStyleConfiguration
        let selected: Bool
        @Environment(\.isFocused) private var focused
        @State private var hovered = false

        var body: some View {
            configuration.label
                .labelFont(Theme.type.callout, relativeTo: .callout)
                .foregroundStyle(selected ? Theme.Palette.onPrimary : Theme.Palette.muted)
                .padding(.horizontal, 14)
                .padding(.vertical, 8)
                .background(Capsule().fill(selected ? Theme.Palette.primary : .white.opacity(hovered ? 0.14 : 0.08)))
                .contentShape(Capsule())
                .pressVeil(Capsule(), pressed: configuration.isPressed)
                .modifier(FocusRing(shape: Capsule(), active: focused))
                .scaleEffect(configuration.isPressed ? 0.96 : 1)
                .animation(Theme.Motion.focus, value: configuration.isPressed)
                #if !os(tvOS)
                .onHover { hovered = $0 }
                #endif
        }
    }
}

/// Posters in a grid that fills the width; each opens its title page.
struct PosterGrid: View {
    let tiles: [Tile]
    var caption: (Tile) -> String? = \.releaseInfo
    /// Called when the last poster comes on screen, to load the next page.
    var onEnd: (() -> Void)?
    @Environment(\.metrics) private var metrics

    var body: some View {
        // Columns at about the row poster width, posters stretched to fill them at 2:3.
        LazyVGrid(
            columns: [GridItem(.adaptive(minimum: metrics.poster.width * 0.85), spacing: metrics.wide ? 20 : 12, alignment: .top)],
            alignment: .leading, spacing: metrics.wide ? 28 : 18
        ) {
            ForEach(Array(tiles.enumerated()), id: \.offset) { index, tile in
                NavigationLink(value: TitleRoute(tile)) {
                    VStack(alignment: .leading, spacing: metrics.wide ? 10 : 6) {
                        ArtImage(url: tile.poster, fallback: tile.name)
                            .aspectRatio(2 / 3, contentMode: .fit)
                            .overlay(alignment: .topTrailing) {
                                if let rating = tile.rating { RatingChip(rating: rating).padding(metrics.chip * 0.55) }
                            }
                            .overlay(alignment: .bottom) {
                                if let progress = tile.progress { EdgeProgress(value: progress) }
                            }
                            .cardArt()
                        CardCaption(name: tile.name, info: caption(tile))
                    }
                }
                .buttonStyle(CardButtonStyle())
                .accessibilityLabel([tile.name, caption(tile)].compactMap { $0 }.joined(separator: ", "))
                .onAppear { if index == tiles.count - 1 { onEnd?() } }
            }
        }
    }
}

/// Under card art: the name (700, accent while the card is lit), then the
/// year or the core's caption, dim.
struct CardCaption: View {
    let name: String
    let info: String?
    @Environment(\.metrics) private var metrics

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            CardTitle(text: name, size: metrics.cardName)
            if let info, !info.isEmpty {
                Text(info)
                    .font(.text(metrics.cardInfo, weight: .medium, relativeTo: .caption))
                    .foregroundStyle(Theme.Palette.dim)
                    .lineLimit(1)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// "★ 8.0" on a small strong-glass chip at a poster's top right.
struct RatingChip: View {
    let rating: String
    @Environment(\.metrics) private var metrics

    var body: some View {
        HStack(spacing: metrics.chip * 0.3) {
            Glyph("star-fill", size: metrics.chip * 0.85, relativeTo: .caption2).foregroundStyle(Theme.Palette.star)
            Text(rating).font(.text(metrics.chip, weight: .bold, relativeTo: .caption2)).monospacedDigit()
        }
        .foregroundStyle(Theme.Palette.text)
        .padding(.horizontal, metrics.chip * 0.55)
        .frame(height: metrics.chip * 1.8)
        .glass(Capsule(), strong: true)
        .accessibilityHidden(true)
    }
}

/// The page title in the display face.
struct ScreenTitle: View {
    let text: String
    @Environment(\.metrics) private var metrics

    var body: some View {
        Text(text)
            .display(metrics.wide ? 40 : Theme.type.title1 + 6)
            .foregroundStyle(Theme.Palette.text)
            .accessibilityAddTraits(.isHeader)
    }
}
