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

/// Selected chips are filled with the text color; the rest sit on the raised surface.
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
                .font(.text(Theme.type.callout, weight: .semibold, relativeTo: .callout))
                .foregroundStyle(selected ? Theme.Palette.bg : Theme.Palette.text.opacity(0.85))
                .padding(.horizontal, 14)
                .padding(.vertical, 8)
                .background(Capsule().fill(selected ? Theme.Palette.text : Theme.Palette.raised.opacity(hovered ? 1 : 0.8)))
                .contentShape(Capsule())
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
    var caption: (Tile) -> String? = { _ in nil }
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
                    VStack(alignment: .leading, spacing: 8) {
                        ArtImage(url: tile.poster, fallback: tile.name)
                            .aspectRatio(2 / 3, contentMode: .fit)
                            .overlay(alignment: .bottom) {
                                if let progress = tile.progress { EdgeProgress(value: progress) }
                            }
                            .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous))
                        #if !os(tvOS)
                        // Similar posters need their names; TV shows them on focus instead.
                        Text(tile.name)
                            .font(.text(Theme.type.callout, weight: .medium, relativeTo: .callout))
                            .foregroundStyle(Theme.Palette.text)
                            .lineLimit(2)
                            .multilineTextAlignment(.leading)
                            .frame(maxWidth: .infinity, alignment: .leading)
                        #endif
                        if let text = caption(tile) {
                            Text(text)
                                .font(.text(Theme.type.caption + 1, relativeTo: .caption))
                                .foregroundStyle(Theme.Palette.muted)
                                .lineLimit(1)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
                }
                .buttonStyle(CardButtonStyle())
                .accessibilityLabel([tile.name, caption(tile)].compactMap { $0 }.joined(separator: ", "))
                .onAppear { if index == tiles.count - 1 { onEnd?() } }
            }
        }
    }
}

/// The page title in the display face.
struct ScreenTitle: View {
    let text: String
    @Environment(\.metrics) private var metrics

    var body: some View {
        Text(text)
            .font(.display(metrics.wide ? 40 : Theme.type.title1 + 6))
            .foregroundStyle(Theme.Palette.text)
            .accessibilityAddTraits(.isHeader)
    }
}
