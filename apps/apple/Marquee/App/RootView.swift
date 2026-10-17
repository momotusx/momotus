import SwiftUI

enum AppTab: String, CaseIterable, Hashable {
    case home, discover, library, calendar, search, settings

    var title: String { rawValue.capitalized }

    var symbol: String {
        switch self {
        case .home: "house"
        case .discover: "compass"
        case .library: "library-big"
        case .calendar: "calendar"
        case .search: "search"
        case .settings: "settings"
        }
    }

    /// Search and Settings sit after the divider in the top navigation.
    var isTool: Bool { self == .search || self == .settings }
}

/// Moves to another place in the navigation (an empty page's way in), as
/// `openURL` does for links.
struct OpenTabAction {
    let open: @MainActor (AppTab) -> Void

    @MainActor func callAsFunction(_ tab: AppTab) { open(tab) }
}

extension EnvironmentValues {
    @Entry var openTab = OpenTabAction { _ in }
}

/// The floating glass navigation: a bar at the bottom on iPhone and iPad, a
/// pill at the top on the Mac (in the window toolbar) and on TV.
struct RootView: View {
    let home: HomeModel
    @Environment(CoreClient.self) private var core
    @State private var tab = AppTab.home
    @State private var homePath: [TitleRoute] = []
    @State private var immersion = Immersion()
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    /// Tabs opened so far, kept alive so each keeps its place as a tab bar does.
    @State private var opened: Set<AppTab> = [.home]
    @State private var typing = false
    #endif

    var body: some View {
        content
            .preferredColorScheme(.dark)
            .environment(\.metrics, metrics)
            .environment(\.openTab, OpenTabAction { select($0) })
            .environment(immersion)
            // Accent marks focus and the one primary action; everything else takes the text colour.
            .tint(Theme.Palette.text)
            // Home stays subscribed for the app's life, so Back lands on the same rows.
            .task(id: home.taskId) { await home.run() }
            .onAppear(perform: Launch.firstFrame)
            .overlay {
                if let failure = core.failure {
                    CoreStopped(message: failure, reopen: core.restart)
                }
            }
            #if DEBUG
            .onAppear {
                if let route = DebugHooks.openTitle, homePath.isEmpty { homePath = [route] }
                if let start = DebugHooks.tab { select(start) }
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

    @ViewBuilder private var content: some View {
        #if os(iOS)
        ZStack {
            ForEach(AppTab.allCases.filter(opened.contains), id: \.self) { item in
                screen(item)
                    .opacity(item == tab ? 1 : 0)
                    .allowsHitTesting(item == tab)
                    .accessibilityHidden(item != tab)
            }
        }
        .safeAreaInset(edge: .bottom, spacing: 0) {
            // Off while the keyboard is up, as a tab bar sits behind it.
            if !immersion.active && !typing {
                TabBar(selection: tab, select: select)
            }
        }
        .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillShowNotification)) { _ in typing = true }
        .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillHideNotification)) { _ in typing = false }
        #elseif os(macOS)
        screen(tab)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Theme.Palette.bg)
            .toolbar { NavToolbar(selection: tab, select: select) }
        #else
        screen(tab).safeAreaInset(edge: .top, spacing: 0) {
            if !immersion.active {
                TopNav(selection: tab, select: select).padding(.bottom, Theme.Space.m)
            }
        }
        #endif
    }

    private func select(_ item: AppTab) {
        #if os(iOS)
        opened.insert(item)
        #endif
        tab = item
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
        case .calendar:
            NavigationStack { CalendarScreen(core: core).withTitleRoutes(core: core) }
        case .settings:
            NavigationStack { SettingsScreen(core: core) }
        }
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

/// "● Marquee": the accent dot and the name in display type.
struct Wordmark: View {
    var size: CGFloat = Theme.type.title3

    var body: some View {
        HStack(spacing: size * 0.38) {
            Circle().fill(Theme.Palette.accent).frame(width: size * 0.38, height: size * 0.38)
            Text("Marquee").display(size)
        }
        .foregroundStyle(Theme.Palette.text)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Marquee")
        .accessibilityAddTraits(.isHeader)
    }
}

#if os(iOS)
/// A floating glass bar at the bottom: icons over their words, the current
/// place on a lighter rounded square.
private struct TabBar: View {
    let selection: AppTab
    let select: (AppTab) -> Void
    @Environment(\.dynamicTypeSize) private var type
    @State private var width = CGFloat.infinity

    var body: some View {
        let iconsOnly = tabBarIconsOnly(width: width, type: type)
        HStack(spacing: 0) {
            ForEach(AppTab.allCases, id: \.self) { item in
                let on = item == selection
                Button { select(item) } label: {
                    VStack(spacing: 4) {
                        Glyph(item.symbol, size: 22, relativeTo: .caption2)
                        if !iconsOnly {
                            Text(item.title)
                                .font(.text(10.5, weight: .semibold, relativeTo: .caption2))
                                .lineLimit(1)
                                .minimumScaleFactor(0.8)
                        }
                    }
                    .foregroundStyle(on ? Theme.Palette.text : Theme.Palette.dim)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background {
                        if on { RoundedRectangle(cornerRadius: 18, style: .continuous).fill(.white.opacity(0.12)) }
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                // The spoken name, with or without the word on screen.
                .accessibilityLabel(item.title)
                .accessibilityAddTraits(on ? .isSelected : [])
                // A long press at the accessibility sizes shows the name large, as the system bar does.
                .accessibilityShowsLargeContentViewer { Label(item.title, glyph: item.symbol) }
            }
        }
        .padding(6)
        .frame(maxWidth: 560)
        .frame(height: 64)
        // The icons grow with the text up to the size that still fits the bar.
        .dynamicTypeSize(...DynamicTypeSize.xxxLarge)
        .glass(RoundedRectangle(cornerRadius: 24, style: .continuous), shadow: true)
        .padding(.horizontal, 12)
        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
        .padding(.bottom, 2)
    }
}
#endif

#if os(macOS)
/// The navigation pill in the middle of the window toolbar, on its own glass.
private struct NavToolbar: ToolbarContent {
    let selection: AppTab
    let select: (AppTab) -> Void

    var body: some ToolbarContent {
        if #available(macOS 26, *) {
            ToolbarItem(placement: .principal) { TopNav(selection: selection, select: select) }
                .sharedBackgroundVisibility(.hidden)
        } else {
            ToolbarItem(placement: .principal) { TopNav(selection: selection, select: select) }
        }
    }
}
#endif

#if !os(iOS)
/// A floating glass pill: the wordmark, the four places, a divider, then
/// Search and Settings with their icons. The current place sits in a
/// lighter inner pill.
private struct TopNav: View {
    let selection: AppTab
    let select: (AppTab) -> Void

    var body: some View {
        HStack(spacing: 2) {
            Wordmark(size: Self.mark)
                .padding(.leading, Self.mark * 0.6)
                .padding(.trailing, Self.mark * 0.4)
            ForEach(AppTab.allCases.filter { !$0.isTool }, id: \.self) { link($0) }
            Capsule().fill(Theme.Palette.lineStrong).frame(width: 1.5, height: Self.mark).padding(.horizontal, 8)
            ForEach(AppTab.allCases.filter(\.isTool), id: \.self) { link($0) }
        }
        .padding(Self.inset)
        .fixedSize()
        .glass(Capsule(), shadow: true)
        #if os(tvOS)
        .focusSection()
        #endif
    }

    private func link(_ item: AppTab) -> some View {
        Button { select(item) } label: {
            if item.isTool { Label(item.title, glyph: item.symbol) } else { Text(item.title) }
        }
        .buttonStyle(NavLinkStyle(on: item == selection))
        .accessibilityAddTraits(item == selection ? .isSelected : [])
    }

    #if os(tvOS)
    private static let mark: CGFloat = 32
    private static let inset: CGFloat = 9
    #else
    private static let mark: CGFloat = 18
    private static let inset: CGFloat = 4
    #endif
}

private struct NavLinkStyle: ButtonStyle {
    let on: Bool

    func makeBody(configuration: Configuration) -> some View {
        Link(configuration: configuration, on: on)
    }

    private struct Link: View {
        let configuration: ButtonStyleConfiguration
        let on: Bool
        @Environment(\.isFocused) private var focused
        @State private var hovered = false

        var body: some View {
            configuration.label
                .labelStyle(NavLabelStyle())
                .font(.text(Theme.type.callout, weight: .semibold, relativeTo: .callout))
                .foregroundStyle(on || focused || hovered ? Theme.Palette.text : Theme.Palette.dim)
                .padding(.horizontal, Self.horizontal)
                .frame(height: Self.height)
                .background {
                    if on {
                        Capsule().fill(.white.opacity(0.12)).overlay(Capsule().strokeBorder(Theme.Palette.edge, lineWidth: 1))
                    }
                }
                .contentShape(Capsule())
                .modifier(FocusRing(shape: Capsule(), active: focused))
                .scaleEffect(focused ? Theme.Focus.scaleCard : configuration.isPressed ? 0.97 : 1)
                .animation(Theme.Motion.focus, value: focused)
                #if !os(tvOS)
                .onHover { hovered = $0 }
                #endif
        }

        #if os(tvOS)
        private static let horizontal: CGFloat = 24
        private static let height: CGFloat = 54
        #else
        private static let horizontal: CGFloat = 14
        private static let height: CGFloat = 32
        #endif
    }
}

private struct NavLabelStyle: LabelStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 7) {
            configuration.icon
            configuration.title
        }
    }
}
#endif

extension View {
    func withTitleRoutes(core: CoreClient) -> some View {
        navigationDestination(for: TitleRoute.self) { route in
            TitleScreen(route: route, core: core)
        }
    }

    /// A scroll-edge material behind the status bar and the floating back
    /// button (iOS): absent on an unscrolled page, it covers content that
    /// scrolls under them. Pages whose art runs under the bar pass `visible`
    /// once content scrolls there.
    func topScrim(_ visible: Bool = true) -> some View {
        #if os(iOS)
        return modifier(TopScrim(visible: visible))
        #else
        return self
        #endif
    }

    /// Lets art run under the window toolbar (macOS) or the status bar (iOS),
    /// as the title page's backdrop and the Mac's home card need. On iOS a
    /// scrim keeps the status bar readable once content scrolls under it.
    func fullBleedTop() -> some View {
        #if os(macOS)
        return self.ignoresSafeArea(edges: .top).toolbarBackgroundVisibility(.hidden, for: .windowToolbar)
        #elseif os(iOS)
        // No scrim of its own: each backdrop puts one between its art and its
        // words, and `topScrim` takes over once content scrolls under the bar.
        return self.ignoresSafeArea(edges: .top)
        #else
        return self
        #endif
    }
}

#if os(iOS)
/// iOS 26 and later: the system's hard scroll-edge material, shown only once
/// content has scrolled under the bar (it would frost a hero's art at rest).
/// Earlier systems: a page-coloured fade.
private struct TopScrim: ViewModifier {
    let visible: Bool
    @State private var scrolled = false

    func body(content: Content) -> some View {
        if #available(iOS 26, *) {
            content
                .onScrollGeometryChange(for: Bool.self) { $0.contentOffset.y + $0.contentInsets.top > 1 } action: { scrolled = $1 }
                .scrollEdgeEffectStyle(.hard, for: .top)
                .scrollEdgeEffectHidden(!(visible && scrolled), for: .top)
        } else {
            content.overlay(alignment: .top) {
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
        }
    }
}
#endif

/// Presents a flow over everything: full screen on iOS and tvOS (no tab bar,
/// no navigation pill), a sheet on macOS.
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
                .display(Theme.type.title1)
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
