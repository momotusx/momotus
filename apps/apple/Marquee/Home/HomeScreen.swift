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
    @FocusState private var addFocused: Bool

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
    /// One way on, which takes the focus on TV.
    private var setup: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            if !metrics.wide {
                Wordmark(size: 22).padding(.bottom, Theme.Space.l)
            }
            Text("Home fills up from your addons")
                .display(Theme.type.title1)
                .foregroundStyle(Theme.Palette.text)
            Text("None of the addons you have on shows a catalog here. Add one, or turn rows back on in Settings.")
                .font(.text(Theme.type.body))
                .foregroundStyle(Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
            NavigationLink { AddonsPage(core: core) } label: { Label("Add an addon", glyph: "puzzle") }
                .buttonStyle(PillButtonStyle(prominent: true))
                .focused($addFocused)
                .padding(.top, 4)
        }
        .padding(.top, metrics.wide ? Theme.Space.xxl : Theme.Space.xxl - 20)
        .defaultFocus($addFocused, true)
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
