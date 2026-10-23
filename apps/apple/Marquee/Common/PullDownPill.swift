import SwiftUI

/// A pull-down drawn as a pill: the title panel's season menu (15) and Discover's filters on the
/// Mac and iPad (13, the frame's). White at 10 percent, `chevron-down`, `control.pill` tall.
struct PullDownPill<Items: View>: View {
    let title: String
    var size: CGFloat = 15
    @ViewBuilder let items: () -> Items
    @Environment(\.metrics) private var metrics

    var body: some View {
        Menu(content: items) {
            HStack(spacing: 6) {
                Text(title)
                Glyph("chevron-down", size: 14)
            }
            .textFont(size, weight: .semibold, relativeTo: size > 13 ? .subheadline : .footnote)
            .foregroundStyle(Theme.Palette.text)
            .padding(.horizontal, 14)
            .frame(height: metrics.wide ? Theme.Pill.desktop : Theme.Pill.phone)
            .background(.white.opacity(0.1), in: Capsule())
        }
        .menuStyle(.button)
        .buttonStyle(.plain)
        .menuIndicator(.hidden)
        .fixedSize()
    }
}
