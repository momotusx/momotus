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
    /// A page that moves focus itself (TV Find addons) binds its chips here.
    var focus: FocusState<ID?>.Binding?
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
        Text(caps: label)
            .textFont(Theme.type.caption, weight: .semibold, relativeTo: .caption)
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
            .chipFocus(focus, chip.id)
        }
    }
}

private extension View {
    @ViewBuilder func chipFocus<ID: Hashable>(_ binding: FocusState<ID?>.Binding?, _ id: ID) -> some View {
        if let binding { focused(binding, equals: id) } else { self }
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
        @KeyboardFocused private var focused
        @State private var hovered = false
        @Environment(\.accessibilityReduceMotion) private var reduceMotion

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
                .scaleEffect(configuration.isPressed && !reduceMotion ? 0.96 : 1)
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
            let keyed = tiles.keyed
            ForEach(keyed, id: \.key) { key, tile in
                NavigationLink(value: TitleRoute(tile)) {
                    VStack(alignment: .leading, spacing: metrics.wide ? 10 : 6) {
                        ArtImage(url: tile.poster, fallback: tile.name)
                            .aspectRatio(2 / 3, contentMode: .fit)
                            .overlay(alignment: .topTrailing) {
                                if !ratingParts(tile.ratings).isEmpty { RatingChip(ratings: ratingParts(tile.ratings)).padding(metrics.chip * 0.55) }
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
                .onAppear { if key == keyed.last?.key { onEnd?() } }
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
                    .textFont(metrics.cardInfo, weight: .medium, relativeTo: .caption)
                    .foregroundStyle(Theme.Palette.dim)
                    .lineLimit(1)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
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
