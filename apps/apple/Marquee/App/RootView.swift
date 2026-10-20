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
    /// The tab this view is in is the one on screen. On a phone every opened
    /// tab stays alive behind the shown one, so its pages never disappear.
    @Entry var tabShown = true
    /// Bumped by Cmd-F on the Mac: Search puts the cursor in its field.
    @Entry var searchRequest = 0
}

/// The floating glass navigation: a bar at the bottom on iPhone and iPad, a
/// pill at the top on the Mac (in the window toolbar) and on TV.
///
/// The profile gate: nothing of a profile shows before the core says which
/// one runs and whether it is locked. While the gate is shut only the picker
/// shows; each switch builds the profile's screens again from nothing.
struct RootView: View {
    @Environment(CoreClient.self) private var core
    @Environment(ProfileSession.self) private var profiles
    @Environment(DebridSession.self) private var debrid
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    /// A film playing on in Picture in Picture while the viewer browses.
    @State private var floating = FloatingPlayer()
    #endif

    var body: some View {
        Group {
            if profiles.open && debrid.ready {
                MainShell(core: core).id(profiles.epoch)
            } else if core.failure != nil || !debrid.ready {
                // The core stopped (only its message and the way back show,
                // in the overlay), or its debrid accounts are not back yet.
                Theme.Palette.bg.ignoresSafeArea()
            } else if profiles.snapshot != nil {
                ProfilePicker(mode: .gate, core: core)
            } else if let problem = profiles.loadProblem {
                // The profiles view is opened again every 2 seconds meanwhile.
                VStack(spacing: Theme.Space.s) {
                    Text("Profiles did not load").display(Theme.type.title1).foregroundStyle(Theme.Palette.text)
                    Text(problem).font(.text(Theme.type.body)).foregroundStyle(Theme.Palette.muted).multilineTextAlignment(.center)
                    Label("Trying again", glyph: "refresh-cw").font(.text(Theme.type.callout, relativeTo: .callout)).foregroundStyle(Theme.Palette.muted)
                }
                .padding(Theme.Space.l)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(Theme.Palette.bg)
            } else {
                Theme.Palette.bg.ignoresSafeArea()
            }
        }
        .preferredColorScheme(.dark)
        .environment(\.metrics, metrics)
        // Accent marks focus and the one primary action; everything else takes the text colour.
        .tint(Theme.Palette.text)
        // The profiles view, again after a core restart.
        .task(id: profiles.view.taskId) { profiles.view.start() }
        .onAppear(perform: Launch.firstFrame)
        #if os(iOS)
        .environment(floating)
        // A profile switch or a lock closes the floating window with the screens.
        .onChange(of: [profiles.epoch, profiles.open ? 1 : 0]) { floating.end() }
        #endif
        .overlay {
            if let failure = core.failure {
                CoreStopped(message: failure, reopen: core.restart)
            }
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

/// One profile's app: the tabs, their navigation and Home's model, all built
/// again when the profile changes.
struct MainShell: View {
    @Environment(CoreClient.self) private var core
    @State private var home: HomeModel
    @State private var tab = AppTab.home
    @State private var homePath: [TitleRoute] = []
    @State private var settingsPath: [SettingsPage] = []
    /// The title pages over Search, Discover, Library and Calendar.
    @State private var titlePaths: [AppTab: [TitleRoute]] = [:]
    @State private var immersion = Immersion()
    @Environment(ProfileSession.self) private var profiles
    /// Home has scrolled off its top row (TV Back takes it there first).
    @State private var homeScrolled = false
    /// Bumped to send Home's focus back to its top row.
    @State private var homeTop = 0
    #if !os(iOS)
    /// The profile picker, from the avatar at the end of the pill.
    @State private var showProfiles = false
    #endif
    #if os(tvOS)
    /// Places whose root page has another page over it: Back pops that first.
    @State private var covered: Set<AppTab> = []
    /// The pill's height with its gap, which TV Search keeps clear.
    @State private var navHeight: CGFloat = 0
    #endif
    /// Places opened so far, kept alive so each keeps its place (a query, a
    /// catalog and its scroll, a filter, an open title) as a tab bar does.
    @State private var opened: Set<AppTab> = [.home]
    /// Cmd-F presses, for Search's field.
    @State private var searchRequests = 0
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    @State private var typing = false
    @Environment(FloatingPlayer.self) private var floating
    #endif

    init(core: CoreClient) {
        _home = State(initialValue: HomeModel(core: core))
    }

    var body: some View {
        content
            .environment(\.metrics, metrics)
            .environment(\.openTab, OpenTabAction { select($0) })
            .environment(immersion)
            // Home stays subscribed for the profile's life, so Back lands on the same rows.
            .task(id: home.taskId) { await home.run() }
            // A switch waits until these screens have left.
            .onAppear { profiles.shell(onScreen: true) }
            .onDisappear { profiles.shell(onScreen: false) }
            #if DEBUG
            .onAppear {
                if let route = DebugHooks.openTitle, homePath.isEmpty { homePath = [route] }
                if let start = DebugHooks.tab { select(start) }
                if let page = DebugHooks.takeSettingsPage() {
                    select(.settings)
                    settingsPath = [page]
                }
            }
            .task {
                for url in DebugHooks.installAddons {
                    do {
                        try await core.perform("installAddon", ["url": url])
                    } catch {
                        // The host only: an addon link can carry a key in its path.
                        let host = URL(string: url)?.host() ?? "?"
                        coreLog.notice("debug install from \(host, privacy: .public): \(error.localizedDescription, privacy: .public)")
                    }
                }
            }
            #endif
    }

    @ViewBuilder private var content: some View {
        #if os(iOS)
        places
        // The bar's inset below does not reach scroll views inside the tabs'
        // navigation stacks (a page's end stayed under the bar): every page,
        // pushed or not, keeps the bar's height free at its end.
        .contentMargins(.bottom, TabBarMetrics.height, for: .scrollContent)
        .safeAreaInset(edge: .bottom, spacing: 0) {
            // Off while the keyboard is up, as a tab bar sits behind it.
            if !immersion.active && !typing {
                TabBar(selection: tab, select: select)
            }
        }
        .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillShowNotification)) { _ in typing = true }
        .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillHideNotification)) { _ in typing = false }
        // The film brought back from Picture in Picture, over whichever place is open.
        .fullScreenCover(item: Bindable(floating).shown) { held in
            NavigationStack { PlayerScreen(held: held, core: core) }
        }
        .overlay(alignment: .top) {
            if let note = floating.note {
                Text(note)
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.text)
                    .padding(.horizontal, Theme.Space.m)
                    .padding(.vertical, Theme.Space.s)
                    .glass(Capsule(), shadow: true)
                    .padding(.top, Theme.Space.s)
                    .transition(.opacity)
            }
        }
        .animation(Theme.Motion.panel, value: floating.note)
        #elseif os(macOS)
        places
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Theme.Palette.bg)
            .toolbar { NavToolbar(selection: tab, select: select, profiles: { showProfiles = true }) }
            .modifier(FullScreen(isPresented: $showProfiles) {
                NavigationStack {
                    ProfilePicker(mode: .push, core: core)
                        // A sheet: Escape or Done leaves it as it was.
                        .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Done") { showProfiles = false } } }
                }
                .environment(profiles)
                .environment(\.metrics, metrics)
            })
            // The Go menu and its keys, off while the player covers the places.
            .focusedSceneValue(\.places, immersion.active ? nil : OpenTabAction { select($0) })
            .focusedSceneValue(\.find, immersion.active ? nil : OpenTabAction { _ in
                // A title open over Search goes first: the field is on Search's first page.
                popToRoot(.search)
                select(.search)
                searchRequests += 1
            })
        #else
        places.safeAreaInset(edge: .top, spacing: 0) {
            // Top-level places only, as on Android: a pushed page has no pill to wander into.
            if !immersion.active && !pageOver {
                TopNav(selection: tab, select: select, profiles: { showProfiles = true }).padding(.bottom, Theme.Space.m)
                    .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { navHeight = $0 }
            }
        }
        .modifier(FullScreen(isPresented: $showProfiles) {
            NavigationStack { ProfilePicker(mode: .push, core: core) }
                .environment(profiles)
                .environment(\.metrics, metrics)
        })
        // nil leaves Menu to the system: a pushed page pops, the top of Home exits.
        .onExitCommand(perform: back)
        #endif
    }

    #if os(tvOS)
    /// A page is over this place's root. Home's path counts too: a page put
    /// there before Home's root appeared never marked it covered.
    private var pageOver: Bool { covered.contains(tab) || (tab == .home && !homePath.isEmpty) }

    /// Menu on a place's root page: another place goes Home, Home scrolled down
    /// goes to its top row, else nothing here and the app exits.
    private var back: (() -> Void)? {
        guard !pageOver else { return nil }
        switch TVBack.step(atHome: tab == .home, scrolledDown: homeScrolled) {
        case .goHome: return { select(.home) }
        case .toTop: return { homeTop += 1 }
        case .exit: return nil
        }
    }
    #endif

    private func select(_ item: AppTab) {
        #if os(iOS)
        // The current tab again goes back to its first page, as a tab bar does.
        if item == tab { popToRoot(item) }
        #endif
        opened.insert(item)
        tab = item
    }

    private func popToRoot(_ item: AppTab) {
        switch item {
        case .home: homePath = []
        case .settings: settingsPath = []
        default: titlePaths[item] = []
        }
    }

    private func titlePath(_ item: AppTab) -> Binding<[TitleRoute]> {
        Binding { titlePaths[item] ?? [] } set: { titlePaths[item] = $0 }
    }

    /// Every opened place, the current one shown. The others stay alive but
    /// out of sight, touch, VoiceOver and (on TV) focus, so leaving a place
    /// and coming back finds it as it was (review apple-journey P2-2).
    private var places: some View {
        ZStack {
            ForEach(AppTab.allCases.filter(opened.contains), id: \.self) { item in
                screen(item)
                    .environment(\.tabShown, item == tab)
                    .environment(\.searchRequest, searchRequests)
                    .opacity(item == tab ? 1 : 0)
                    .allowsHitTesting(item == tab)
                    .accessibilityHidden(item != tab)
                    #if os(tvOS)
                    .disabled(item != tab)
                    #endif
            }
        }
    }

    @ViewBuilder private func screen(_ tab: AppTab) -> some View {
        switch tab {
        case .home:
            NavigationStack(path: $homePath) {
                HomeScreen(model: home, topRequest: homeTop, scrolled: $homeScrolled)
                    .root(of: .home, covered: coveredBinding)
                    .withTitleRoutes(core: core)
                    #if os(iOS)
                    .toolbar(.hidden, for: .navigationBar)
                    #endif
            }
        case .search:
            NavigationStack(path: titlePath(.search)) { SearchScreen(core: core).root(of: .search, covered: coveredBinding).withTitleRoutes(core: core) }
                #if os(tvOS)
                // The system's search keyboard lays itself out from the top of the
                // screen, past the inset: it starts below the pill.
                .padding(.top, !immersion.active && !pageOver ? navHeight : 0)
                .background(Theme.Palette.bg.ignoresSafeArea())
                #endif
        case .discover:
            NavigationStack(path: titlePath(.discover)) { DiscoverScreen(core: core).root(of: .discover, covered: coveredBinding).withTitleRoutes(core: core) }
        case .library:
            NavigationStack(path: titlePath(.library)) { LibraryScreen(core: core).root(of: .library, covered: coveredBinding).withTitleRoutes(core: core) }
        case .calendar:
            NavigationStack(path: titlePath(.calendar)) { CalendarScreen(core: core).root(of: .calendar, covered: coveredBinding).withTitleRoutes(core: core) }
        case .settings:
            NavigationStack(path: $settingsPath) { SettingsScreen(core: core).root(of: .settings, covered: coveredBinding) }
        }
    }

    /// TV: which places have a page over their root (none elsewhere).
    private var coveredBinding: Binding<Set<AppTab>> {
        #if os(tvOS)
        $covered
        #else
        .constant([])
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

extension View {
    /// TV: marks a place's root page, so Back knows when another page is over it.
    @ViewBuilder func root(of tab: AppTab, covered: Binding<Set<AppTab>>) -> some View {
        #if os(tvOS)
        onAppear { covered.wrappedValue.remove(tab) }
            .onDisappear { covered.wrappedValue.insert(tab) }
        #else
        self
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
    let profiles: () -> Void

    var body: some ToolbarContent {
        if #available(macOS 26, *) {
            ToolbarItem(placement: .principal) { TopNav(selection: selection, select: select, profiles: profiles) }
                .sharedBackgroundVisibility(.hidden)
        } else {
            ToolbarItem(placement: .principal) { TopNav(selection: selection, select: select, profiles: profiles) }
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
    /// The active profile's avatar at the end, opening the picker.
    var profiles: (() -> Void)?
    /// Which link has focus: Up from a page enters on the current place, not the nearest link.
    @FocusState private var focused: AppTab?

    var body: some View {
        HStack(spacing: 2) {
            Wordmark(size: Self.mark)
                .padding(.leading, Self.mark * 0.6)
                .padding(.trailing, Self.mark * 0.4)
            ForEach(AppTab.allCases.filter { !$0.isTool }, id: \.self) { link($0) }
            Capsule().fill(Theme.Palette.lineStrong).frame(width: 1.5, height: Self.mark).padding(.horizontal, 8)
            ForEach(AppTab.allCases.filter(\.isTool), id: \.self) { link($0) }
            if let profiles { ProfileButton(size: Self.mark + 8, action: profiles).padding(.leading, 6) }
        }
        .padding(Self.inset)
        .fixedSize()
        .glass(Capsule(), shadow: true)
        #if os(tvOS)
        .focusSection()
        .defaultFocus($focused, selection, priority: .userInitiated)
        #endif
    }

    private func link(_ item: AppTab) -> some View {
        Button { select(item) } label: {
            if item.isTool { Label(item.title, glyph: item.symbol) } else { Text(item.title) }
        }
        .buttonStyle(NavLinkStyle(on: item == selection))
        .focused($focused, equals: item)
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

struct NavLinkStyle: ButtonStyle {
    let on: Bool

    func makeBody(configuration: Configuration) -> some View {
        Link(configuration: configuration, on: on)
    }

    private struct Link: View {
        let configuration: ButtonStyleConfiguration
        let on: Bool
        @KeyboardFocused private var focused
        @State private var hovered = false
        @Environment(\.accessibilityReduceMotion) private var reduceMotion

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
                .pressVeil(Capsule(), pressed: configuration.isPressed)
                .modifier(FocusRing(shape: Capsule(), active: focused))
                .scaleEffect(reduceMotion ? 1 : focused ? Theme.Focus.scaleCard : configuration.isPressed ? 0.97 : 1)
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
    /// `atRest`: shown before any scroll too, for a page whose words can start under the bar.
    /// Every page has this top chrome, so on the Mac it also keeps the back
    /// button of a page whose place is out of sight off the shared window
    /// toolbar (review apple-journey round 2 P2-B).
    func topScrim(_ visible: Bool = true, atRest: Bool = false) -> some View {
        #if os(iOS)
        return modifier(TopScrim(visible: visible, atRest: atRest))
        #elseif os(macOS)
        return modifier(BackOnlyWhenShown())
        #else
        return self
        #endif
    }

    /// Lets art run under the window toolbar (macOS) or the status bar (iOS),
    /// as the title page's backdrop and the Mac's home card need. On iOS a
    /// scrim keeps the status bar readable once content scrolls under it.
    func fullBleedTop() -> some View {
        #if os(macOS)
        return modifier(FullBleedTop())
        #elseif os(iOS)
        // No scrim of its own: each backdrop puts one between its art and its
        // words, and `topScrim` takes over once content scrolls under the bar.
        return self.ignoresSafeArea(edges: .top)
        #else
        return self
        #endif
    }
}

#if os(macOS)
/// The window toolbar is one for every place: a page of a place kept out of
/// sight leaves no back button there, which would pop a page nobody sees.
private struct BackOnlyWhenShown: ViewModifier {
    @Environment(\.tabShown) private var shown

    func body(content: Content) -> some View {
        content.navigationBarBackButtonHidden(!shown)
    }
}

/// The window toolbar is one for every place: a place kept alive out of sight
/// must not clear its background for the one on screen.
private struct FullBleedTop: ViewModifier {
    @Environment(\.tabShown) private var shown

    func body(content: Content) -> some View {
        content
            .ignoresSafeArea(edges: .top)
            .toolbarBackgroundVisibility(shown ? .hidden : .automatic, for: .windowToolbar)
    }
}
#endif

#if os(iOS)
/// iOS 26 and later: the system's hard scroll-edge material, shown only once
/// content has scrolled under the bar (it would frost a hero's art at rest),
/// or at rest when the page says its words already sit under the bar.
/// Earlier systems: a page-coloured fade.
private struct TopScrim: ViewModifier {
    let visible: Bool
    var atRest = false
    @State private var scrolled = false

    func body(content: Content) -> some View {
        if #available(iOS 26, *) {
            content
                .onScrollGeometryChange(for: Bool.self) { $0.contentOffset.y + $0.contentInsets.top > 1 } action: { scrolled = $1 }
                .scrollEdgeEffectStyle(.hard, for: .top)
                .scrollEdgeEffectHidden(!(visible && (scrolled || atRest)), for: .top)
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
    /// A place kept alive out of sight presents nothing until it shows again:
    /// on TV it is disabled, and a flow it presented would be disabled too.
    @Environment(\.tabShown) private var shown

    func body(content: Content) -> some View {
        let presented = Binding { isPresented && shown } set: { isPresented = $0 }
        #if os(macOS)
        content.sheet(isPresented: presented) { flow().frame(minWidth: 760, minHeight: 640) }
        #else
        content.fullScreenCover(isPresented: presented, content: flow)
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
