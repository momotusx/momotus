import SwiftUI

/// Moves to another place in the navigation (an empty page's way in), as
/// `openURL` does for links.
struct OpenTabAction {
    let open: @MainActor (AppTab, Bool) -> Void

    /// `link`: a title page's chip asks for the place, which only its first page answers.
    @MainActor func callAsFunction(_ tab: AppTab, link: Bool = false) { open(tab, link) }
}

/// Pushes a title on this place's stack: Discover opens its selected poster this way, after a first
/// activation only selected it.
struct OpenTitleAction {
    let open: @MainActor (TitleRoute) -> Void

    @MainActor func callAsFunction(_ route: TitleRoute) { open(route) }
}

/// A title page's genre chip (plans/design-v3-phase3-apple.md 4.2): Discover on a kind and a genre.
/// Discover reads the request once it can act on it, then clears it. A director chip writes
/// `SearchInput` instead.
@MainActor @Observable final class LinkRequests {
    var discover: DiscoverRequest?
}

extension EnvironmentValues {
    @Entry var openTab = OpenTabAction { _, _ in }
    @Entry var openTitle = OpenTitleAction { _ in }
    @Entry var linkRequests: LinkRequests? = nil
    /// The tab this view is in is the one on screen. On a phone every opened
    /// tab stays alive behind the shown one, so its pages never disappear.
    @Entry var tabShown = true
    /// The Mac header shows the search field, so Search's page draws none of its own.
    @Entry var headerSearchField = false
}

/// The navigation: the floating glass capsule at the bottom on iPhone and iPad,
/// tv.apple.com's black header in the window toolbar on the Mac, the pill at
/// the top on TV.
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
                    Text(problem).textFont(Theme.type.body).foregroundStyle(Theme.Palette.muted).multilineTextAlignment(.center)
                    Label("Trying again", glyph: "refresh-cw").textFont(Theme.type.callout, relativeTo: .callout).foregroundStyle(Theme.Palette.muted)
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
    /// A genre or a name a title page's chip asked Discover or Search to open.
    @State private var links = LinkRequests()
    #if !os(iOS)
    /// The profile picker, from the avatar at the end of the pill.
    @State private var showProfiles = false
    #endif
    #if os(macOS)
    /// The window's width: the header's search field folds to its glyph on a half-screen window.
    @State private var windowWidth = CGFloat.infinity
    #endif
    #if os(tvOS)
    /// Places whose root page has another page over it: Back pops that first.
    @State private var covered: Set<AppTab> = []
    /// The pill's height with its gap, which TV Search keeps clear.
    @State private var navHeight: CGFloat = 0
    /// The top bar's link with focus (nil: focus is elsewhere). Menu in a place's content puts it on that place.
    @FocusState private var navFocus: AppTab?
    /// The profile avatar at the end of the bar has focus: the bar too, for Menu.
    @FocusState private var avatarFocus: Bool
    /// The places under the bar: Select on the shown link sends focus into the shown one.
    @Namespace private var placesScope
    @Environment(\.resetFocus) private var resetFocus
    #endif
    /// Places opened so far, kept alive so each keeps its place (a query, a
    /// catalog and its scroll, a filter, an open title) as a tab bar does.
    @State private var opened: Set<AppTab> = [.home]
    /// The one search query, shared by the Mac header's field and Search's page.
    @State private var searchInput = SearchInput()
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    @State private var typing = false
    /// The screen's bottom safe-area inset, as the tab bar reads it, for the pages' end margin.
    @State private var bottomInset: CGFloat = 0
    @Environment(FloatingPlayer.self) private var floating
    /// Sign in another device, from the quick action, the /pair link or the profile menu.
    @State private var showScanner = false
    @Environment(SyncModel.self) private var sync
    @Environment(AccountWipe.self) private var wipe
    #endif

    init(core: CoreClient) {
        _home = State(initialValue: HomeModel(core: core))
    }

    var body: some View {
        content
            .environment(\.metrics, metrics)
            .environment(\.openTab, OpenTabAction { select($0, link: $1) })
            .environment(\.linkRequests, links)
            .environment(immersion)
            .environment(searchInput)
            // Typing shows Search's first page, as Cmd-F does.
            .onChange(of: searchInput.text) { _, text in
                if SearchRules.bringsSearch(text: text, onSearch: tab == .search, titleOver: !(titlePaths[.search] ?? []).isEmpty) {
                    popToRoot(.search)
                    select(.search)
                }
            }
            // Home stays subscribed for the profile's life, so Back lands on the same rows.
            .task(id: home.taskId) { await home.run() }
            // A switch waits until these screens have left.
            .onAppear { profiles.shell(onScreen: true) }
            .onDisappear { profiles.shell(onScreen: false) }
            #if DEBUG
            .onAppear {
                if let start = DebugHooks.tab { select(start.lit) }
                // Over the place `-MQTab` named: a title opened from Search, for its chips' walk.
                if let route = DebugHooks.openTitle {
                    if tab == .home, homePath.isEmpty { homePath = [route] }
                    if tab != .home, tab != .settings, titlePaths[tab, default: []].isEmpty { titlePaths[tab] = [route] }
                }
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
                if !DebugHooks.subtitleLanguages.isEmpty {
                    try? await core.perform("setPrefs", ["patch": ["subtitleLanguages": DebugHooks.subtitleLanguages]])
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
        .contentMargins(.bottom, TabBarMetrics.scrollMargin(immersive: immersion.active, bottomInset: bottomInset), for: .scrollContent)
        .safeAreaInset(edge: .bottom, spacing: 0) {
            // Off while the keyboard is up, as a tab bar sits behind it.
            if !immersion.active && !typing {
                TabBar(selection: tab, select: { select($0) }, bottomInset: bottomInset)
            }
        }
        // The screen's own bottom inset (the home indicator), read outside the bar's inset.
        .onGeometryChange(for: CGFloat.self) { $0.safeAreaInsets.bottom } action: { if !typing { bottomInset = $0 } }
        .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillShowNotification)) { _ in typing = true }
        .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillHideNotification)) { _ in typing = false }
        // The film brought back from Picture in Picture, over whichever place is open.
        .fullScreenCover(item: Bindable(floating).shown) { held in
            NavigationStack { PlayerScreen(held: held, core: core) }
        }
        .overlay(alignment: .top) {
            if let note = floating.note {
                Text(note)
                    .textFont(Theme.type.callout, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.text)
                    .padding(.horizontal, Theme.Space.m)
                    .padding(.vertical, Theme.Space.s)
                    // A toast on solid `raised` (design/DECISION.md, States), not glass.
                    .background(Theme.Palette.raised, in: Capsule())
                    .padding(.top, Theme.Space.s)
                    .transition(.opacity)
            }
        }
        .animation(Theme.Motion.panel, value: floating.note)
        // Sign in another device (plans/accounts.md 4.2): the scanner, or the Account page first.
        .sheet(isPresented: $showScanner) {
            NavigationStack {
                if sync.joined { AddDeviceScreen(fromLink: AppRouter.shared.fromLink) } else { SyncScreen() }
            }
            .environment(sync)
            .environment(profiles)
            .environment(core)
            .environment(wipe)
            .environment(\.metrics, metrics)
        }
        // Never on a kids profile: Account and approving are not its own.
        .onChange(of: AppRouter.shared.scanRequests) { if profiles.snapshot?.current?.kids != true { showScanner = true } }
        // A phone camera on a TV's code opens /pair here: the scanner, never the code in the link (D6).
        .onContinueUserActivity(NSUserActivityTypeBrowsingWeb) { activity in
            guard let url = activity.webpageURL, AccountRules.opensScanner(url, linkHost: PasskeyContract.relyingParty) else { return }
            AppRouter.shared.requestScan(fromLink: true)
        }
        #elseif os(macOS)
        places
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Theme.Palette.bg)
            .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { windowWidth = $0 }
            .toolbar {
                NavToolbar(selection: tab, select: { select($0) }, find: find, profiles: { showProfiles = true }, input: searchInput, narrow: windowWidth < NavToolbar.fieldWidthFrom)
            }
            // The field moves between the header and the page: the cursor follows it.
            .onChange(of: windowWidth < NavToolbar.fieldWidthFrom) { if tab == .search { searchInput.focus += 1 } }
            // tv.apple.com's black header.
            .toolbarBackground(Theme.Palette.barDesktop, for: .windowToolbar)
            .toolbarBackgroundVisibility(.visible, for: .windowToolbar)
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
            .focusedSceneValue(\.places, immersion.active ? nil : OpenTabAction { tab, _ in select(tab) })
            .focusedSceneValue(\.find, immersion.active ? nil : OpenTabAction { _, _ in find() })
        #else
        places.safeAreaInset(edge: .top, spacing: 0) {
            // Top-level places only, as on Android: a pushed page has no pill to wander into.
            if !immersion.active && !pageOver {
                let away = TVTopBar.hidden(atHome: tab == .home, scrolledDown: homeScrolled)
                TopNav(selection: tab, select: { select($0) }, enter: { resetFocus(in: placesScope) }, profiles: { showProfiles = true }, focused: $navFocus, avatar: $avatarFocus).padding(.bottom, Theme.Space.m)
                    .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { navHeight = $0 }
                    // Kept in the layout, so the page under it never jumps.
                    .offset(y: away ? -navHeight : 0)
                    .opacity(away ? 0 : 1)
                    .disabled(away)
                    .animation(Theme.Motion.panel, value: away)
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

    /// Menu on a place's root page: another place's content puts focus on its link, its link goes Home, Home scrolled down
    /// goes to its top row, else nothing here and the app exits.
    private var back: (() -> Void)? {
        guard !pageOver else { return nil }
        // The player covering the places leaves no bar to go to: Menu there goes on as before.
        let inBar = navFocus != nil || avatarFocus || immersion.active
        switch TVBack.step(atHome: tab == .home, scrolledDown: homeScrolled, inBar: inBar) {
        case .toBar: return { navFocus = tab }
        case .goHome: return { select(.home) }
        case .toTop: return { homeTop += 1 }
        case .exit: return nil
        }
    }
    #endif

    private func select(_ item: AppTab, link: Bool = false) {
        if item.opensAtRoot(from: tab, link: link) { popToRoot(item) }
        opened.insert(item)
        tab = item
    }

    #if os(macOS)
    /// Cmd-F and the narrow header's glyph: Search's first page with the cursor in its field (a title
    /// open over Search goes first).
    private func find() {
        popToRoot(.search)
        select(.search)
        searchInput.focus += 1
    }
    #endif

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
                    .environment(\.openTitle, OpenTitleAction { route in
                        if item == .home { homePath.append(route) } else { titlePaths[item, default: []].append(route) }
                    })
                    #if os(macOS)
                    .environment(\.headerSearchField, windowWidth >= NavToolbar.fieldWidthFrom)
                    #endif
                    .opacity(item == tab ? 1 : 0)
                    .allowsHitTesting(item == tab)
                    .accessibilityHidden(item != tab)
                    #if os(tvOS)
                    .disabled(item != tab)
                    // The whole place is one target for a move from the top bar: a
                    // page whose rows sit left of the tab in focus is still reached by
                    // Down (the tvOS walk, ATV-F1: Settings).
                    .focusSection()
                    #endif
            }
        }
        #if os(tvOS)
        .focusScope(placesScope)
        #endif
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
            NavigationStack(path: titlePath(.calendar)) {
                CalendarScreen(core: core).root(of: .calendar, covered: coveredBinding).withTitleRoutes(core: core)
                    #if os(iOS)
                    // Reached from Library's Calendar row: the way back to it.
                    .toolbar {
                        ToolbarItem(placement: .topBarLeading) {
                            // A blue back link, the chevron and the word (DECISION Navigation).
                            Button { select(.library) } label: { Label("Library", glyph: "chevron-left") }
                                .labelStyle(.titleAndIcon)
                                .tint(Theme.Palette.key)
                                .foregroundStyle(Theme.Palette.key)
                        }
                    }
                    #endif
            }
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

/// "Marquee": the name in display type, with no dot.
struct Wordmark: View {
    var size: CGFloat = Theme.type.title3

    var body: some View {
        Text("Momotus").display(size)
        .foregroundStyle(Theme.Palette.text)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Momotus")
        .accessibilityAddTraits(.isHeader)
    }
}

#if os(iOS)
/// The iPhone's floating glass capsule at the bottom (apple-tv.md, Frame and navigation): 62 pt
/// tall, 21 pt from the sides and the bottom, five places with their icons over their words, the
/// current one in the key blue on a darker inner capsule.
private struct TabBar: View {
    let selection: AppTab
    let select: (AppTab) -> Void
    let bottomInset: CGFloat
    @Environment(\.dynamicTypeSize) private var type
    @State private var width = CGFloat.infinity

    var body: some View {
        let iconsOnly = tabBarIconsOnly(width: width, type: type)
        HStack(spacing: 0) {
            ForEach(AppTab.bar, id: \.self) { item in
                let on = item == selection.lit
                Button { select(item) } label: {
                    VStack(spacing: 1) {
                        Glyph(item.symbol, size: 26, relativeTo: .caption2)
                        if !iconsOnly {
                            Text(item.title)
                                .textFont(11, weight: .semibold, relativeTo: .caption2)
                                .lineLimit(1)
                                .minimumScaleFactor(0.8)
                        }
                    }
                    .foregroundStyle(on ? Theme.Palette.key : .white)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background {
                        if on { Capsule().fill(Color(white: 0.04).opacity(0.92)) }
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
        .padding(4)
        .frame(maxWidth: 560)
        .frame(height: TabBarMetrics.height)
        // The icons grow with the text up to the size that still fits the bar.
        .dynamicTypeSize(...DynamicTypeSize.xxxLarge)
        .background {
            // The frame's capsule: #202020 at 80 percent over a blur (white labels 8.5:1 over white
            // art on the tint alone), a light rim on the top edge only, and a soft shadow.
            Capsule().fill(Color(white: 32 / 255).opacity(0.8))
                .background(.ultraThinMaterial, in: Capsule())
                .overlay {
                    Capsule().strokeBorder(
                        LinearGradient(stops: [.init(color: .white.opacity(0.22), location: 0), .init(color: .clear, location: 0.12)], startPoint: .top, endPoint: .bottom),
                        lineWidth: 1)
                }
                .shadow(color: .black.opacity(0.45), radius: 15, y: 10)
        }
        .accessibilityElement(children: .contain)
        .accessibilityAddTraits(.isTabBar)
        .padding(.horizontal, 21)
        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
        // The foot 21 pt above the screen's edge: down into the home indicator's band, or up off an SE's edge.
        .padding(.bottom, max(0, -TabBarMetrics.drop(bottomInset: bottomInset)))
        .offset(y: max(0, TabBarMetrics.drop(bottomInset: bottomInset)))
    }
}
#endif

#if os(macOS)
/// tv.apple.com's header in the window toolbar: the wordmark at the left, the places as centered
/// text tabs (16/600, the current one at 92 percent, the others at 64), the search field and the
/// profile at the right. No pill and no glass.
private struct NavToolbar: ToolbarContent {
    let selection: AppTab
    let select: (AppTab) -> Void
    /// The narrow header's glyph: Search's page with the cursor in the page's field.
    let find: () -> Void
    let profiles: () -> Void
    let input: SearchInput
    /// A window too narrow for the 172 pt field beside the tabs: the field is its glyph alone.
    var narrow = false
    /// The narrowest window that holds the wordmark, the centered tabs, the whole field and the
    /// profile, with a pushed page's back button too (measured: at 940 that button cuts the tabs and
    /// the field, at 1000 they only just fit).
    static let fieldWidthFrom: CGFloat = 1060

    var body: some ToolbarContent {
        if #available(macOS 26, *) {
            ToolbarItem(placement: .navigation) { mark }.sharedBackgroundVisibility(.hidden)
            // New items when the width class flips: the toolbar keeps an item's first size otherwise, and
            // the grown field then cuts the tabs.
            ToolbarItem(id: "tabs-\(narrow)", placement: .principal) { tabs }.sharedBackgroundVisibility(.hidden)
            ToolbarItem(id: "tools-\(narrow)", placement: .primaryAction) { tools }.sharedBackgroundVisibility(.hidden)
        } else {
            ToolbarItem(placement: .navigation) { mark }
            ToolbarItem(id: "tabs-\(narrow)", placement: .principal) { tabs }
            ToolbarItem(id: "tools-\(narrow)", placement: .primaryAction) { tools }
        }
    }

    private var mark: some View { Wordmark(size: 17) }

    private var tabs: some View {
        // Closer on a narrow window, so a pushed page's back button fits beside them.
        HStack(spacing: narrow ? 2 : 12) {
            ForEach(AppTab.bar.filter { $0 != .search }, id: \.self) { item in
                Button(item.title) { select(item) }
                    .buttonStyle(DesktopTabStyle(on: item == selection))
                    .accessibilityAddTraits(item == selection ? .isSelected : [])
            }
        }
    }

    private var tools: some View {
        HStack(spacing: 16) {
            if narrow {
                Button(action: find) { Label("Search", glyph: "search") }
                    .buttonStyle(SearchGlyphStyle())
                    // The glyph alone: the pointer still names it.
                    .help("Search")
                    .accessibilityAddTraits(selection == .search ? .isSelected : [])
            } else {
                HeaderSearchField(input: input)
            }
            ProfileButton(size: 28, action: profiles)
        }
    }
}

/// The header's search field (tv.apple.com: 172 by 28, a 1 pt white edge, 15/400): where Search is
/// typed on the Mac. Typing shows Search (`SearchRules.bringsSearch`); Escape clears, then lets go.
private struct HeaderSearchField: View {
    @Bindable var input: SearchInput
    @FocusState private var focused: Bool

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 4, style: .continuous)
        HStack(spacing: 6) {
            Glyph("search", size: 13).foregroundStyle(Theme.Palette.dim)
            TextField("Search", text: $input.text, prompt: Text("Search").foregroundStyle(Theme.Palette.dim))
                .textFieldStyle(.plain)
                .foregroundStyle(Theme.Palette.text)
                .autocorrectionDisabled()
                .focused($focused)
                // The core searches as you type: Return only moves to the first result.
                .onSubmit { input.submit += 1 }
                .onKeyPress(.escape) {
                    switch SearchRules.escape(input.text) {
                    case .clear: input.text = ""
                    case .leave: focused = false
                    }
                    return .handled
                }
        }
        .textFont(15, relativeTo: .callout)
        .padding(.horizontal, 8)
        .frame(width: 172, height: 28)
        .overlay(shape.strokeBorder(Theme.Palette.text, lineWidth: 1))
        .contentShape(shape)
        .modifier(FocusRing(shape: shape, active: focused))
        .onChange(of: input.focus) { focused = true }
    }
}

/// A header tab: words only, brighter when current or under the pointer, the key ring for keyboard focus.
private struct DesktopTabStyle: ButtonStyle {
    let on: Bool

    func makeBody(configuration: Configuration) -> some View {
        Tab(configuration: configuration, on: on)
    }

    private struct Tab: View {
        let configuration: ButtonStyleConfiguration
        let on: Bool
        @KeyboardFocused private var focused
        @State private var hovered = false

        var body: some View {
            let shape = RoundedRectangle(cornerRadius: 6, style: .continuous)
            configuration.label
                .textFont(16, weight: .semibold, relativeTo: .callout)
                .foregroundStyle(on || hovered ? Theme.Palette.text : Theme.Palette.dim)
                .padding(.horizontal, 6)
                .frame(height: 28)
                .contentShape(shape)
                .modifier(FocusRing(shape: shape, active: focused))
                .onHover { hovered = $0 }
        }
    }
}

/// The narrow header's search: a 28 pt square with the glyph alone and the field's 1 pt edge.
private struct SearchGlyphStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        Field(configuration: configuration)
    }

    private struct Field: View {
        let configuration: ButtonStyleConfiguration
        @KeyboardFocused private var focused

        var body: some View {
            let shape = RoundedRectangle(cornerRadius: 4, style: .continuous)
            configuration.label.labelStyle(.iconOnly)
                .textFont(15, relativeTo: .callout)
                .foregroundStyle(Theme.Palette.dim)
                .frame(width: 28, height: 28)
                .overlay(shape.strokeBorder(Theme.Palette.text, lineWidth: 1))
                .contentShape(shape)
                .modifier(FocusRing(shape: shape, active: focused))
        }
    }
}
#endif

#if os(tvOS)
/// TV's floating glass pill: the wordmark, the four places, a divider, then
/// Search and Settings with their icons. The current place sits in a
/// lighter inner pill. tvOS keeps it as it is (D9).
private struct TopNav: View {
    let selection: AppTab
    let select: (AppTab) -> Void
    /// Select on the shown place's link: focus goes into the place, as the tvOS tab bar does.
    let enter: () -> Void
    /// The active profile's avatar at the end, opening the picker.
    var profiles: (() -> Void)?
    /// Which link has focus: Up from a page enters on the current place, not the nearest link. A link that
    /// keeps it for the settle shows its place, as the tvOS tab bar does; focus stays on it.
    var focused: FocusState<AppTab?>.Binding
    /// The avatar has focus (Menu there goes Home, as on a link).
    var avatar: FocusState<Bool>.Binding

    var body: some View {
        HStack(spacing: 2) {
            Wordmark(size: Self.mark)
                .padding(.leading, Self.mark * 0.6)
                .padding(.trailing, Self.mark * 0.4)
            ForEach(AppTab.allCases.filter { !$0.isTool }, id: \.self) { link($0) }
            Capsule().fill(Theme.Palette.lineStrong).frame(width: 1.5, height: Self.mark).padding(.horizontal, 8)
            ForEach(AppTab.allCases.filter(\.isTool), id: \.self) { link($0) }
            if let profiles { ProfileButton(size: Self.mark + 8, action: profiles).focused(avatar).padding(.leading, 6) }
        }
        .padding(Self.inset)
        .fixedSize()
        .glass(Capsule(), shadow: true)
        #if os(tvOS)
        .focusSection()
        .defaultFocus(focused, selection, priority: .userInitiated)
        // 250 ms, the androidx.tv TabRow's debounce: a sweep across the bar opens no place on the way.
        .task(id: focused.wrappedValue) {
            guard let item = focused.wrappedValue, item != selection else { return }
            try? await Task.sleep(for: .milliseconds(250))
            guard !Task.isCancelled else { return }
            select(item)
        }
        // The place changed from elsewhere while focus sat on another link (Menu on the bar went Home): focus follows
        // it, so the settle never takes the viewer back. The bar's own switch finds focus on that link already.
        .onChange(of: selection) { _, now in
            if let item = focused.wrappedValue, item != now { focused.wrappedValue = now }
        }
        #endif
    }

    private func link(_ item: AppTab) -> some View {
        Button { item == selection ? enter() : select(item) } label: {
            if item.isTool { Label(item.title, glyph: item.symbol) } else { Text(item.title) }
        }
        .buttonStyle(NavLinkStyle(on: item == selection))
        .focused(focused, equals: item)
        .accessibilityAddTraits(item == selection ? .isSelected : [])
    }

    private static let mark: CGFloat = 32
    private static let inset: CGFloat = 9
}
#endif

#if !os(iOS)
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
                .textFont(Theme.type.callout, weight: .semibold, relativeTo: .callout)
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

    /// Lets art run under the status bar (iOS), as the title page's backdrop
    /// needs; a scrim keeps the status bar readable once content scrolls under
    /// it. The Mac's header is tv.apple.com's black bar on every page, so there
    /// the art starts under it.
    func fullBleedTop() -> some View {
        #if os(iOS)
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
            Text("Momotus stopped")
                .display(Theme.type.title1)
            Text(message)
                .textFont(Theme.type.body)
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
