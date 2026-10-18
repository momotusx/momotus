import SwiftUI

/// Home: hero, then Continue Watching and catalog rows in core order.
struct HomeScreen: View {
    let model: HomeModel
    @Environment(CoreClient.self) private var core
    @Environment(\.metrics) private var metrics
    @Environment(SyncModel.self) private var sync
    @Environment(ProfileSession.self) private var profiles
    @State private var width: CGFloat = 0
    @State private var onboarding = false
    @State private var showProfiles = false
    /// Rows have scrolled under the status bar.
    @State private var underBar = false

    var body: some View {
        ScrollView(.vertical) {
            LazyVStack(alignment: .leading, spacing: metrics.rowSpacing) {
                // Without catalogs there is no art to show: setup takes the top.
                if !model.needsSetup {
                    HeroView(hero: model.header.hero, height: heroHeight)
                        // Phone: the profile button sits at the header's top right.
                        .overlay(alignment: .topTrailing) {
                            if !metrics.wide { ProfileButton { showProfiles = true }.padding(.trailing, 24).padding(.top, 10) }
                        }
                        .padding(.top, cardTop)
                }
                content
            }
            .padding(.bottom, Theme.Space.xl)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .onScrollGeometryChange(for: Bool.self) { $0.contentOffset.y + $0.contentInsets.top > cardTop } action: { underBar = $1 }
        #if os(macOS)
        // The card runs up under the window toolbar, where the navigation pill floats over it.
        .fullBleedTop()
        #endif
        .topScrim(underBar || model.needsSetup)
        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
        #if DEBUG
        .onAppear { if DebugHooks.takeOnboarding() { onboarding = true } }
        #endif
        .navigationDestination(isPresented: $showProfiles) { ProfilePicker(mode: .push, core: core) }
        // A restarted core builds Home again: onboarding opens again if it was showing.
        .onAppear { if profiles.resumeOnboarding { onboarding = true } }
        .onChange(of: onboarding) { _, open in profiles.resumeOnboarding = open }
        .modifier(FullScreen(isPresented: $onboarding) {
            NavigationStack {
                OnboardingScreen(core: core) { onboarding = false }
            }
            .environment(\.metrics, metrics)
            .environment(sync)
        })
    }

    @ViewBuilder private var content: some View {
        switch model.phase {
        case .opening:
            // The core may still be opening: the shell shows at once, rows to come.
            ForEach(0..<3, id: \.self) { _ in SkeletonRow() }
        case .failed(let message):
            StateLine(symbol: "triangle-alert", text: message, action: ("Try again", model.reload))
                .padding(.horizontal, metrics.gutter)
        case .ready:
            ForEach(model.rows, id: \.rowId) { row in
                RowView(row: row, retry: model.reload)
            }
            if model.needsSetup { setup.padding(.horizontal, metrics.gutter) }
        }
    }

    /// No rows: no addon that is on has a catalog, or every row is hidden.
    private var setup: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            if !metrics.wide {
                Wordmark(size: 22).padding(.bottom, Theme.Space.l)
            }
            CatalogSetup(
                title: "Home fills up from your addons",
                text: "None of the addons you have on shows a catalog here. Set up Marquee adds the standard catalog in one step, or add an addon yourself.",
                core: core, setUp: { onboarding = true })
        }
        .padding(.top, metrics.wide ? Theme.Space.xxl : Theme.Space.xxl - 20)
    }

    /// The card's gap to the top: under the status bar on a phone, under
    /// the window toolbar on the Mac.
    private var cardTop: CGFloat {
        #if os(macOS)
        return 12
        #elseif os(tvOS)
        return 0
        #else
        return 8
        #endif
    }

    private var heroHeight: CGFloat {
        #if os(tvOS)
        return 600
        #else
        guard width > 0 else { return metrics.wide ? 460 : 520 }
        return metrics.wide ? min(max(width * 0.36, 400), 560) : 520
        #endif
    }
}

/// No catalog to show (empty Home and Discover): why, then Set up Marquee,
/// whose Start fresh adds the standard catalog in one press with no typing
/// on a remote, and Add an addon. On TV the focus enters on Set up Marquee.
struct CatalogSetup: View {
    let title: String
    let text: String
    var titleSize = Theme.type.title1
    let core: CoreClient
    let setUp: () -> Void
    @FocusState private var primary: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            Text(title)
                .display(titleSize)
                .foregroundStyle(Theme.Palette.text)
                .accessibilityAddTraits(.isHeader)
            Text(text)
                .font(.text(Theme.type.body))
                .foregroundStyle(Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
            FlowRow(spacing: 12) {
                Button(action: setUp) { Label("Set up Marquee", glyph: "wrench") }
                    .buttonStyle(PillButtonStyle(prominent: true))
                    .focused($primary)
                NavigationLink { AddonsPage(core: core) } label: { Label("Add an addon", glyph: "puzzle") }
                    .buttonStyle(PillButtonStyle())
            }
            .padding(.top, 4)
        }
        .defaultFocus($primary, true)
    }
}
