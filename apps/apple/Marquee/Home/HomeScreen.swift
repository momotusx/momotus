import SwiftUI

/// Home: hero, then Continue Watching and catalog rows in core order.
struct HomeScreen: View {
    let model: HomeModel
    @Environment(CoreClient.self) private var core
    @Environment(\.metrics) private var metrics
    @State private var width: CGFloat = 0

    var body: some View {
        ScrollView(.vertical) {
            LazyVStack(alignment: .leading, spacing: metrics.rowSpacing) {
                // Without catalogs there is no art to show: setup takes the top.
                if !model.needsSetup {
                    HeroView(hero: model.header.hero, height: heroHeight, overlap: overlap)
                }
                content
            }
            .padding(.bottom, Theme.Space.xl)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .fullBleedTop()
        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
    }

    @ViewBuilder private var content: some View {
        switch model.phase {
        case .opening:
            EmptyView()
        case .failed(let message):
            StateLine(symbol: "exclamationmark.triangle", text: message, action: ("Try again", model.reload))
                .padding(.horizontal, metrics.gutter)
        case .ready:
            ForEach(model.rows, id: \.rowId) { row in
                RowView(row: row, retry: model.reload)
            }
            if model.needsSetup { setup.padding(.horizontal, metrics.gutter) }
        }
    }

    /// The first run: no catalogs yet, so Home points to setup.
    private var setup: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            if !metrics.wide {
                Text("Marquee").font(.display(Theme.type.title1)).foregroundStyle(Theme.Palette.muted)
                    .padding(.bottom, Theme.Space.l)
            }
            Text("Home fills up from your addons")
                .font(.display(Theme.type.title1))
                .foregroundStyle(Theme.Palette.text)
            Text("No addon with catalogs is installed yet. Add one to see what is popular, new and featured.")
                .font(.text(Theme.type.body))
                .foregroundStyle(Theme.Palette.muted)
            FlowRow(spacing: 12) {
                NavigationLink { OnboardingScreen(core: core) } label: { Label("Set up Marquee", systemImage: "wrench.and.screwdriver") }
                    .buttonStyle(PillButtonStyle(prominent: true))
                NavigationLink { AddonsPage(core: core) } label: { Label("Addons", systemImage: "puzzlepiece.extension") }
                    .buttonStyle(PillButtonStyle())
            }
        }
        .padding(.top, metrics.wide ? Theme.Space.xxl : Theme.Space.xxl - 20)
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
