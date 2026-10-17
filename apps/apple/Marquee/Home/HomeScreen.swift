import SwiftUI

/// Home: hero, then Continue Watching and catalog rows in core order.
struct HomeScreen: View {
    let model: HomeModel
    @Environment(\.metrics) private var metrics
    @State private var width: CGFloat = 0

    var body: some View {
        ScrollView(.vertical) {
            LazyVStack(alignment: .leading, spacing: metrics.rowSpacing) {
                HeroView(hero: model.header.hero, height: heroHeight, overlap: overlap)
                content
            }
            .padding(.bottom, Theme.Space.xl)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .fullBleedTop()
        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
        .task(id: model.taskId) { await model.run() }
    }

    @ViewBuilder private var content: some View {
        switch model.phase {
        case .opening:
            EmptyView()
        case .settingUp:
            StateLine(symbol: "shippingbox", text: "Installing Cinemeta, the free catalog of movies and series.")
                .padding(.horizontal, metrics.gutter)
        case .failed(let message):
            StateLine(symbol: "exclamationmark.triangle", text: message, action: ("Try again", model.reload))
                .padding(.horizontal, metrics.gutter)
        case .ready:
            if model.rows.isEmpty {
                StateLine(symbol: "square.stack", text: "No catalogs yet. Install an addon to fill Home.")
                    .padding(.horizontal, metrics.gutter)
            }
            ForEach(model.rows, id: \.rowId) { row in
                RowView(row: row, retry: model.reload)
            }
        }
    }

    /// The first row slides up over the faded bottom of the hero art.
    private var overlap: CGFloat { metrics.wide ? 72 : 24 }

    private var heroHeight: CGFloat {
        #if os(tvOS)
        return 670
        #else
        guard width > 0 else { return metrics.wide ? 520 : 460 }
        return metrics.wide ? min(max(width * 0.5, 420), 640) : min(width * 1.2, 540)
        #endif
    }
}
