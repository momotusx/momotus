import SwiftUI

enum AppTab: String, CaseIterable, Hashable {
    case search, home, discover, library, settings

    var title: String { rawValue.capitalized }

    var symbol: String {
        switch self {
        case .search: "search"
        case .home: "house"
        case .discover: "compass"
        case .library: "library-big"
        case .settings: "settings"
        }
    }
}

/// Labeled navigation: a sidebar on macOS and iPad, a tab bar on iPhone, the
/// top tab bar on tvOS.
struct RootView: View {
    let home: HomeModel
    @Environment(CoreClient.self) private var core
    @State private var tab = AppTab.home
    @State private var homePath: [TitleRoute] = []
    @State private var columns = NavigationSplitViewVisibility.all
    @State private var immersion = Immersion()
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    #endif

    var body: some View {
        Group {
            if metrics.wide && !isTV {
                NavigationSplitView(columnVisibility: Binding(
                    get: { immersion.active ? .detailOnly : columns }, set: { columns = $0 }
                )) {
                    Sidebar(selection: $tab)
                        .navigationSplitViewColumnWidth(220)
                } detail: {
                    screen(tab)
                }
            } else {
                TabView(selection: $tab) {
                    ForEach(AppTab.allCases, id: \.self) { item in
                        Tab(item.title, image: item.symbol, value: item) { screen(item) }
                    }
                }
            }
        }
        #if os(tvOS)
        .preferredColorScheme(.dark)
        #endif
        .environment(\.metrics, metrics)
        .environment(immersion)
        // Accent marks focus and the one primary action; everything else, the tab bar too, takes the text colour.
        .tint(Theme.Palette.text)
        // Home stays subscribed for the app's life, so Back lands on the same rows.
        .task(id: home.taskId) { await home.run() }
        .overlay {
            if let failure = core.failure {
                CoreStopped(message: failure, reopen: core.restart)
            }
        }
        #if DEBUG
        .onAppear {
            if let route = DebugHooks.openTitle, homePath.isEmpty { homePath = [route] }
            if let start = DebugHooks.tab { tab = start }
        }
        .task {
            for url in DebugHooks.installAddons {
                do {
                    try await core.perform("installAddon", ["url": url])
                } catch {
                    coreLog.notice("debug install \(url, privacy: .public): \(error.localizedDescription, privacy: .public)")
                }
            }
        }
        #endif
    }

    @ViewBuilder private func screen(_ tab: AppTab) -> some View {
        switch tab {
        case .home:
            NavigationStack(path: $homePath) {
                HomeScreen(model: home)
                    .withTitleRoutes(core: core)
                    #if os(iOS)
                    .toolbar(.hidden, for: .navigationBar)
                    #endif
            }
        case .search:
            NavigationStack { SearchScreen(core: core).withTitleRoutes(core: core) }
        case .discover:
            NavigationStack { DiscoverScreen(core: core).withTitleRoutes(core: core) }
        case .library:
            NavigationStack { LibraryScreen(core: core).withTitleRoutes(core: core) }
        case .settings:
            NavigationStack { SettingsScreen(core: core) }
        }
    }

    private var isTV: Bool {
        #if os(tvOS)
        true
        #else
        false
        #endif
    }

    private var metrics: Metrics {
        #if os(tvOS)
        return .tv
        #elseif os(macOS)
        return .regular
        #else
        return sizeClass == .regular ? .regular : .compact
        #endif
    }
}

/// The wordmark and the five destinations, each an icon with its word.
private struct Sidebar: View {
    @Binding var selection: AppTab

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("Marquee")
                .font(.display(Theme.type.title1))
                .foregroundStyle(Theme.Palette.text)
                .padding(.leading, 18)
                .padding(.bottom, Theme.Space.m)
                .accessibilityAddTraits(.isHeader)
            ForEach(AppTab.allCases, id: \.self) { tab in
                Button {
                    selection = tab
                } label: {
                    Label(tab.title, glyph: tab.symbol)
                }
                .buttonStyle(SidebarItemStyle(selected: tab == selection))
                .accessibilityAddTraits(tab == selection ? .isSelected : [])
            }
            Spacer()
        }
        .padding(.top, Theme.Space.l)
        .padding(.horizontal, 12)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(Theme.Palette.bg)
        #if os(macOS)
        .toolbar(removing: .sidebarToggle)
        #endif
    }
}

private struct SidebarItemStyle: ButtonStyle {
    let selected: Bool

    func makeBody(configuration: Configuration) -> some View {
        Item(configuration: configuration, selected: selected)
    }

    private struct Item: View {
        let configuration: ButtonStyleConfiguration
        let selected: Bool
        @State private var hovered = false
        @Environment(\.isFocused) private var focused

        var body: some View {
            configuration.label
                .font(.text(Theme.type.headline, weight: selected ? .semibold : .regular, relativeTo: .headline))
                .foregroundStyle(selected ? Theme.Palette.text : Theme.Palette.muted)
                .padding(.vertical, 9)
                .padding(.horizontal, 12)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(
                    RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous)
                        .fill(Theme.Palette.surface.opacity(configuration.isPressed ? 1 : hovered ? 0.7 : 0)))
                .overlay(alignment: .leading) {
                    if selected {
                        Capsule().fill(Theme.Palette.text).frame(width: 3, height: 18).offset(x: -6)
                    }
                }
                .contentShape(Rectangle())
                .modifier(FocusRing(shape: RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous), active: focused))
                .animation(Theme.Motion.focus, value: hovered)
                #if !os(tvOS)
                .onHover { hovered = $0 }
                #endif
        }
    }
}

extension View {
    func withTitleRoutes(core: CoreClient) -> some View {
        navigationDestination(for: TitleRoute.self) { route in
            TitleScreen(route: route, core: core)
        }
    }

    /// A page-coloured scrim behind the status bar and the floating back
    /// button (iOS), as the tab bar has its material at the bottom: invisible
    /// on an unscrolled page, it hides content that scrolls under them.
    /// Pages whose art runs under the bar pass `visible` once content scrolls there.
    func topScrim(_ visible: Bool = true) -> some View {
        #if os(iOS)
        return overlay(alignment: .top) {
            // Solid down to the bar's bottom (the safe area: status bar, plus the
            // back button on pushed pages), then a 24 pt fade.
            GeometryReader { geo in
                let bar = geo.safeAreaInsets.top
                LinearGradient(
                    stops: [
                        .init(color: Theme.Palette.bg, location: 0),
                        .init(color: Theme.Palette.bg.opacity(0.92), location: bar / (bar + 24)),
                        .init(color: Theme.Palette.bg.opacity(0), location: 1),
                    ],
                    startPoint: .top, endPoint: .bottom)
                    .frame(height: bar + 24)
                    // The reader starts under the bar; the scrim belongs on it.
                    .offset(y: -bar)
            }
            .allowsHitTesting(false)
            .opacity(visible ? 1 : 0)
            .animation(Theme.Motion.focus, value: visible)
        }
        #else
        return self
        #endif
    }

    /// Lets art run under the window toolbar (macOS) or the status bar (iOS),
    /// as the hero needs. On iOS a scrim keeps the status bar readable once
    /// content scrolls under it.
    func fullBleedTop() -> some View {
        #if os(macOS)
        return self.ignoresSafeArea(edges: .top).toolbarBackgroundVisibility(.hidden, for: .windowToolbar)
        #elseif os(iOS)
        // No scrim of its own: each hero puts one between its art and its words,
        // and `topScrim` takes over once content scrolls under the bar.
        return self.ignoresSafeArea(edges: .top)
        #else
        return self
        #endif
    }
}

/// Presents a flow over everything: full screen on iOS and tvOS (no tab bar,
/// no nav rail), a sheet on macOS.
struct FullScreen<Flow: View>: ViewModifier {
    @Binding var isPresented: Bool
    @ViewBuilder let flow: () -> Flow

    func body(content: Content) -> some View {
        #if os(macOS)
        content.sheet(isPresented: $isPresented) { flow().frame(minWidth: 760, minHeight: 640) }
        #else
        content.fullScreenCover(isPresented: $isPresented, content: flow)
        #endif
    }
}

/// Shown when the core could not open or stopped, with the way back.
private struct CoreStopped: View {
    let message: String
    let reopen: () -> Void

    var body: some View {
        VStack(spacing: Theme.Space.m) {
            Text("Marquee stopped")
                .font(.display(Theme.type.title1))
            Text(message)
                .font(.text(Theme.type.body))
                .foregroundStyle(Theme.Palette.muted)
                .multilineTextAlignment(.center)
            Button("Open again", action: reopen)
                .buttonStyle(PillButtonStyle(prominent: true))
        }
        .padding(Theme.Space.xl)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Theme.Palette.bg)
        .foregroundStyle(Theme.Palette.text)
    }
}
