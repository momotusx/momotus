import SwiftUI

/// One choice in a chip row.
struct Chip<ID: Hashable>: Identifiable {
    let id: ID
    let title: String
    var detail: String?
}

/// "Show  Movies  Series": a plain label and its chips. Wraps on wide
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
        Text(label)
            .textFont(Theme.type.callout, weight: .semibold, relativeTo: .callout)
            .foregroundStyle(Theme.Palette.dim)
    }

    @ViewBuilder private var buttons: some View {
        ForEach(chips) { chip in
            Button { select(chip.id) } label: {
                HStack(spacing: 6) {
                    // Selection is a fill and a check, never a ring: the ring means focus.
                    if isSelected(chip.id) { Glyph("check", size: Theme.type.callout, relativeTo: .callout) }
                    Text(chip.title)
                    if let detail = chip.detail {
                        Text(detail).fontWeight(.regular)
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

extension ChipFill {
    static var trackColor: Color { Color(red: track.r / 255, green: track.g / 255, blue: track.b / 255) }
}

/// The segmented control's chips: the selected one on `selected.fill` with white words and its
/// check, the rest on the track grey (`ChipFill`). A fill, never an edge.
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
                .foregroundStyle(selected ? Theme.Palette.text : Theme.Palette.muted)
                .padding(.horizontal, 14)
                .padding(.vertical, 8)
                .background(Capsule().fill(selected ? Theme.Palette.selectedFill : track))
                .contentShape(Capsule())
                .pressVeil(Capsule(), pressed: configuration.isPressed)
                .modifier(FocusRing(shape: Capsule(), active: focused))
                .scaleEffect(configuration.isPressed && !reduceMotion ? 0.96 : 1)
                .animation(Theme.Motion.focus, value: configuration.isPressed)
                #if !os(tvOS)
                .onHover { hovered = $0 }
                #endif
        }

        private var track: Color { ChipFill.trackColor.opacity(hovered ? ChipFill.hover : ChipFill.rest) }
    }
}

/// The grid every poster screen lays its covers in.
private func posterColumns(_ metrics: Metrics) -> [GridItem] {
    [GridItem(.adaptive(minimum: metrics.poster.width * 0.85), spacing: metrics.wide ? 20 : 12, alignment: .top)]
}

/// Posters in a grid that fills the width; each opens its title page.
struct PosterGrid: View {
    let tiles: [Tile]
    var caption: (Tile) -> String? = \.releaseInfo
    /// Called when the last poster comes on screen, to load the next page.
    var onEnd: (() -> Void)?
    /// A small label on a poster's top corner (Discover's sort value).
    var corner: (Tile) -> String? = { _ in nil }
    /// The focused poster's key, for a screen that must know focus is in the grid (Discover on TV).
    var focused: FocusState<String?>.Binding?
    @Environment(\.metrics) private var metrics

    var body: some View {
        // Columns at about the row poster width, posters stretched to fill them at 2:3.
        LazyVGrid(
            columns: posterColumns(metrics),
            alignment: .leading, spacing: metrics.wide ? 28 : 18
        ) {
            let keyed = tiles.keyed
            ForEach(keyed, id: \.key) { key, tile in
                NavigationLink(value: TitleRoute(tile)) {
                    VStack(alignment: .leading, spacing: metrics.wide ? 10 : 6) {
                        // The cover carries the ratings band (RPDB's); the addon's poster when RPDB has none.
                        ArtImage(url: tile.poster, fallbackURL: tile.posterFallback, fallback: tile.name)
                            .aspectRatio(2 / 3, contentMode: .fit)
                            .overlay(alignment: .bottom) {
                                if let progress = tile.progress { EdgeProgress(value: progress) }
                            }
                            .overlay { if let value = corner(tile) { CornerLabel(text: value, banded: tile.posterFallback != nil) } }
                            .cardArt()
                        CardCaption(name: tile.name, info: caption(tile))
                    }
                }
                .buttonStyle(CardButtonStyle())
                .modifier(FocusedKey(binding: focused, key: key))
                .accessibilityLabel(posterLabel([tile.name, corner(tile), caption(tile)], tile.ratings))
                .onAppear { if key == keyed.last?.key { onEnd?() } }
            }
        }
    }
}

/// Covers still coming: blank posters in the grid's places, no words.
struct PosterGridSkeleton: View {
    @Environment(\.metrics) private var metrics

    var body: some View {
        LazyVGrid(columns: posterColumns(metrics), alignment: .leading, spacing: metrics.wide ? 28 : 18) {
            ForEach(0..<12, id: \.self) { _ in
                RoundedRectangle(cornerRadius: Theme.Tile.radius, style: .continuous)
                    .fill(Theme.Palette.surface)
                    .aspectRatio(2 / 3, contentMode: .fit)
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Loading")
    }
}

/// Under card art: the name (600, never recolored by focus), then the year or
/// the core's caption at 64 percent, regular.
struct CardCaption: View {
    let name: String
    let info: String?
    @Environment(\.metrics) private var metrics

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            CardTitle(text: name, size: metrics.cardName)
            if let info, !info.isEmpty {
                Text(info)
                    .textFont(metrics.cardInfo, relativeTo: .caption)
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
    /// The desktop frames' 28 on the Mac and iPad: Discover and Search only, for now.
    var desktop = false
    @Environment(\.metrics) private var metrics

    var body: some View {
        Text(text)
            .display(metrics.screenTitle(desktop: desktop))
            .foregroundStyle(Theme.Palette.text)
            .accessibilityAddTraits(.isHeader)
    }
}

/// Binds a view's focus to `key` when a screen asks for it.
private struct FocusedKey: ViewModifier {
    let binding: FocusState<String?>.Binding?
    let key: String

    func body(content: Content) -> some View {
        if let binding { content.focused(binding, equals: key) } else { content }
    }
}

/// The value a Discover list is ordered by ("IMDb 8.4", "2024"), compact, at a poster's bottom right: just above
/// RPDB's ratings band (about a tenth of the cover, `banded`) so the title art and the band's scores stay clear.
struct CornerLabel: View {
    let text: String
    var banded = false
    @Environment(\.metrics) private var metrics

    var body: some View {
        GeometryReader { g in
            Text(text)
                .textFont(metrics.wide ? 11 : 10, weight: .semibold, relativeTo: .caption2)
                .foregroundStyle(Theme.Palette.text)
                .padding(.horizontal, 5)
                .padding(.vertical, 2)
                .background(Theme.Palette.glassStrong, in: RoundedRectangle(cornerRadius: 5, style: .continuous))
                .padding(.trailing, 5)
                // ponytail: an RPDB cover that fails shows the addon's poster, and the label then sits a tenth above
                // its foot; knowing which loaded means passing the image's state up here.
                .padding(.bottom, (banded ? g.size.height * 0.105 : 0) + 5)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomTrailing)
        }
        .accessibilityHidden(true)
    }
}
