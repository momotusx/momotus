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
    #if !os(tvOS)
    /// A page of Library to open (the offline banner asks for Downloads); Library clears it.
    var library: LibraryRoute?
    #endif
}

extension EnvironmentValues {
    @Entry var openTab = OpenTabAction { _, _ in }
    @Entry var openTitle = OpenTitleAction { _ in }
    @Entry var linkRequests: LinkRequests? = nil
    /// Something is over Home's first page (a pushed page, the player): its hero holds its pages and its trailer.
    @Entry var heroCovered = false
}

/// The navigation: the floating glass capsule at the bottom on iPhone and iPad,
/// the glass ornament at the window's foot on the Mac, the pill at the top on TV.
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
    #if os(macOS)
    @State private var windowClose = WindowClose()
    #endif

    var body: some View {
        Group {
            if profiles.open && debrid.ready && profiles.intro {
                IncognitoIntro()
            } else if profiles.open && debrid.ready {
                // Built again on a switch, and when Incognito starts or ends: its screens read it once.
                MainShell(core: core).id("\(profiles.epoch)/\(profiles.incognito)")
                    .overlay(alignment: .bottom) { notices }
            } else if core.failure != nil {
                // The core stopped: only its message and the way back show, in the overlay.
                Theme.Palette.bg.ignoresSafeArea()
            } else if debrid.ready, profiles.snapshot != nil {
                ProfilePicker(mode: .gate, core: core)
            } else if debrid.ready, let problem = profiles.loadProblem {
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
                // The core opening, its debrid accounts coming back, the profiles answering: one starting
                // screen through all of them, so the launch screen's bird never leaves.
                Starting()
            }
        }
        .preferredColorScheme(.dark)
        .environment(\.metrics, metrics)
        // Accent marks focus and the one primary action; everything else takes the text colour.
        .tint(Theme.Palette.text)
        // The profiles view, again after a core restart.
        .task(id: profiles.view.taskId) { profiles.view.start() }
        .onAppear(perform: Launch.firstFrame)
        #if os(macOS)
        // Closing the main window ends Incognito (5.3); the app itself may stay for downloads.
        .background(WindowReader { window in windowClose.watch(window) { profiles.leaveOnClose() } })
        #endif
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

    /// Over the profile's screens, at the foot above the bar: the ended line over a single profile's Home (5.4), and
    /// the Mac's one-minute warning (2.7).
    private var notices: some View {
        VStack(spacing: Theme.Space.s) {
            if profiles.snapshot?.incognitoEnded == true {
                IncognitoEndedNotice(ok: profiles.ackEnded)
            }
            #if os(macOS)
            if profiles.endsSoon {
                IncognitoEndsSoon(stay: profiles.stay)
            }
            #endif
        }
        .padding(.horizontal, metrics.gutter)
        .padding(.bottom, 96)
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
    /// The profile picker over the places: the profile's menu opens it on a profile's keypad (`pinFor`).
    @State private var showProfiles = false
    @State private var pinFor: ProfileInfo?
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
    /// Sign in another device, from the quick action or the /pair link (Account has its own row).
    @State private var showScanner = false
    @Environment(SyncModel.self) private var sync
    @Environment(AccountWipe.self) private var wipe
    #endif
    #if !os(tvOS)
    @Environment(SystemEvents.self) private var system: SystemEvents?
    @Environment(DownloadKeeper.self) private var keeper: DownloadKeeper?
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
            // Incognito's typing teaches the keyboard nothing (plans/batch-10-incognito.md 5.5).
            .autocorrectionDisabled(profiles.incognito)
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
                        DebugHooks.note("install from \(URL(string: url)?.host() ?? "?") done")
                    } catch {
                        // The host only: an addon link can carry a key in its path.
                        let host = URL(string: url)?.host() ?? "?"
                        coreLog.notice("debug install from \(host, privacy: .public): \(String(describing: error), privacy: .public)")
                        DebugHooks.note("install from \(host) failed: \(error.localizedDescription)")
                    }
                }
                if let patch = DebugHooks.prefsPatch {
                    try? await core.perform("setPrefs", ["patch": patch])
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
        .modifier(offline)
        // The app switcher shows nothing of an Incognito session (5.5).
        .modifier(IncognitoCovering(on: profiles.incognito, playerOnScreen: immersion.active || floating.shown != nil))
        // The bar's inset below does not reach scroll views inside the tabs'
        // navigation stacks (a page's end stayed under the bar): every page,
        // pushed or not, keeps the bar's height free at its end.
        .contentMargins(.bottom, TabBarMetrics.scrollMargin(immersive: immersion.active, bottomInset: bottomInset), for: .scrollContent)
        .safeAreaInset(edge: .bottom, spacing: 0) {
            // Off while the keyboard is up, as a tab bar sits behind it.
            if !immersion.active && !typing {
                TabBar(selection: tab, select: { select($0) }, pin: pin, bottomInset: bottomInset)
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
                if AccountRules.scanOpensApprove(joined: sync.joined, kids: profiles.snapshot?.current?.kids == true) { AddDeviceScreen() } else { SyncScreen() }
            }
            .environment(sync)
            .environment(profiles)
            .environment(core)
            .environment(wipe)
            .environment(\.metrics, metrics)
        }
        // The profile picker on a keypad the profile's menu chose; Done or Cancel leaves it as it was.
        .modifier(FullScreen(isPresented: $showProfiles) {
            NavigationStack {
                ProfilePicker(mode: .push, core: core, pinFor: pinFor)
                    .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Done") { showProfiles = false } } }
            }
            .environment(profiles)
            .environment(\.metrics, metrics)
        })
        // On a kids profile the sheet only says to ask a grown-up: approving is not its own.
        // initial: a link or quick action that started the app arrived before this shell existed.
        .onChange(of: AppRouter.shared.scanRequests, initial: true) {
            guard AppRouter.shared.unanswered else { return }
            AppRouter.shared.unanswered = false
            showScanner = true
        }
        // A phone camera on a TV's code opens /pair here: the approve sheet with that code (plans/tv-qr-login.md 5.6).
        .onContinueUserActivity(NSUserActivityTypeBrowsingWeb) { activity in
            guard let url = activity.webpageURL, let code = AccountRules.pairingCode(url, linkHost: PasskeyContract.relyingParty) else { return }
            AppRouter.shared.requestScan(code: code)
        }
        #elseif os(macOS)
        places
            .modifier(offline)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Theme.Palette.bg)
            // The owner's variant 4: the places in a glass ornament at the window's foot; pages end above it.
            .placeOrnament(tab, strong: ornamentStrong, shown: !immersion.active, select: { select($0) }) {
                // Settings' first page, from any of its pages too (a link opens a place at its root).
                ProfileMenu(onSettings: { select(.settings, link: true) }, onPin: pin) { me in
                    OrnamentProfile(lit: tab == .settings) { BarAvatar(profile: me, size: OrnamentLayout.itemHeight - 8) }
                }
                .help("Profiles and Settings")
                .accessibilityAddTraits(tab == .settings ? .isSelected : [])
            }
            // Content under the traffic lights and the top buttons gets a soft blurred edge once it scrolls there.
            .overlay(alignment: .top) { ScrollTopEdge().opacity(ornamentStrong && !immersion.active ? 1 : 0).animation(Theme.Motion.panel, value: ornamentStrong) }
            .toolbar { TopTools(find: find) }
            // No strip: the window's toolbar draws nothing of its own, and Home's art runs under it.
            .toolbarBackgroundVisibility(.hidden, for: .windowToolbar)
            .modifier(FullScreen(isPresented: $showProfiles, closesOnBack: true) {
                NavigationStack {
                    ProfilePicker(mode: .push, core: core, pinFor: pinFor)
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
                TopNav(selection: tab, select: { select($0) }, enter: { resetFocus(in: placesScope) }, settings: { select(.settings, link: true) }, pin: pin, focused: $navFocus, avatar: $avatarFocus).padding(.bottom, Theme.Space.m)
                    .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { navHeight = $0 }
                    // Kept in the layout, so the page under it never jumps.
                    .offset(y: away ? -navHeight : 0)
                    .opacity(away ? 0 : 1)
                    .disabled(away)
                    .animation(Theme.Motion.panel, value: away)
            }
        }
        .modifier(FullScreen(isPresented: $showProfiles) {
            NavigationStack { ProfilePicker(mode: .push, core: core, pinFor: pinFor) }
                .environment(profiles)
                .environment(\.metrics, metrics)
        })
        // nil leaves Menu to the system: a pushed page pops, the top of Home exits.
        .onExitCommand(perform: back)
        #endif
    }

    #if !os(tvOS)
    /// The offline banner over every place (plans/offline-downloads.md 3.5): Go to Downloads opens Library's
    /// Downloads page unless one is on screen.
    private var offline: OfflineShell {
        let shown = OfflineBannerRule.shows(offline: system?.offline == true, tv: false, immersed: immersion.active)
        let onDownloads = tab == .library && !(keeper?.downloadsPages.isEmpty ?? true)
        let offers = OfflineBannerRule.offersDownloads(keeper?.snapshot, onDownloads: onDownloads)
        return OfflineShell(shown: shown, gutter: metrics.gutter, open: offers ? {
            links.library = .downloads
            select(.library, link: true)
        } : nil)
    }
    #endif

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
        case .toBar: return { if let link = tab.barFocus { navFocus = link } else { avatarFocus = true } }
        case .goHome: return { select(.home) }
        case .toTop: return { homeTop += 1 }
        case .exit: return nil
        }
    }
    #endif

    /// The profile's menu chose a profile with a PIN: the picker opens over the places on its keypad.
    private func pin(_ profile: ProfileInfo) {
        pinFor = profile
        showProfiles = true
    }

    private func select(_ asked: AppTab, link: Bool = false) {
        // Discover and Calendar are not a kids profile's, from a bar, a menu key or a chip.
        let item = asked.opened(kids: profiles.snapshot?.current?.kids == true)
        if item.opensAtRoot(from: tab, link: link) { popToRoot(item) }
        opened.insert(item)
        tab = item
    }

    #if os(macOS)
    /// The ornament's glass darkens once anything but Home's art at rest lies behind it.
    private var ornamentStrong: Bool { tab != .home || homeScrolled || !homePath.isEmpty }

    /// Cmd-F and the search button: Search's first page with the cursor in its field (a title open over
    /// Search goes first).
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
                    .environment(\.heroCovered, immersion.active || !homePath.isEmpty || showProfiles)
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
            NavigationStack(path: $settingsPath) { SettingsScreen(core: core, home: home).root(of: .settings, covered: coveredBinding) }
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

/// B (design/logo, plans/owner-picks.md): the owner's motmot before the name, Unbounded SemiBold outlined
/// in mint, where the name helps. `size` is the name's old point size: the bird is 1.45 of it.
struct Wordmark: View {
    var size: CGFloat = Theme.type.title3
    /// False where text beside it already says the name (About's version line).
    var spoken = true

    var body: some View {
        let bird = size * 1.45, word = bird * 0.45
        HStack(spacing: bird * 0.2) {
            BrandMark(size: bird)
            Image(decorative: "Wordmark").resizable().aspectRatio(contentMode: .fit).frame(height: word)
        }
        // Its letters sit on the line of the text beside it.
        .alignmentGuide(.firstTextBaseline) { $0[VerticalAlignment.center] + word / 2 }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Momotus")
        .accessibilityAddTraits(.isHeader)
        .accessibilityHidden(!spoken)
    }
}

/// C: the bird alone, where room is tight. Not spoken: the place around it says what it is.
struct BrandMark: View {
    var size: CGFloat

    var body: some View {
        Image(decorative: "Mark").resizable().interpolation(.high).frame(width: size, height: size)
    }
}

/// B while the app starts: a faint copy with the solid one revealed from the left, its edge feathered, over and
/// over, as the loading screen fills a title's logo (design/DECISION.md, Loading). Solid under Reduce Motion.
struct BrandLoading: View {
    var size: CGFloat
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var start = Date()

    var body: some View {
        Group {
            if reduceMotion {
                Wordmark(size: size)
            } else {
                TimelineView(.animation) { context in
                    let feather = 0.03
                    let t = context.date.timeIntervalSince(start).truncatingRemainder(dividingBy: 2.4) / 2.4
                    let p = -feather + t * (1 + 2 * feather)
                    ZStack {
                        Wordmark(size: size).opacity(0.2)
                        Wordmark(size: size).mask {
                            LinearGradient(
                                stops: [.init(color: .black, location: min(max(p - feather, 0), 1)),
                                        .init(color: .clear, location: min(max(p + feather, 0), 1))],
                                startPoint: .leading, endPoint: .trailing)
                        }
                    }
                }
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Momotus is starting")
    }
}

/// What shows while the core opens and the profiles answer: the launch screen's bird (LaunchMark, 120 pt, in the
/// middle of the whole screen) from the first frame, so the hand-over shows nothing move, then after half a second
/// the bird and the name filling, so a slow start never looks stuck.
private struct Starting: View {
    @Environment(\.metrics) private var metrics
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var filling = false

    var body: some View {
        ZStack {
            Theme.Palette.bg
            if filling {
                BrandLoading(size: metrics.wide ? 56 : 40).transition(.opacity)
            } else {
                BrandMark(size: 120).transition(.opacity)
            }
        }
        .ignoresSafeArea()
        .task {
            try? await Task.sleep(for: .milliseconds(500))
            withAnimation(reduceMotion ? nil : .easeInOut(duration: 0.35)) { filling = true }
        }
    }
}

#if os(iOS)
/// The iPhone's floating glass capsule at the bottom (apple-tv.md, Frame and navigation): 62 pt
/// tall, 21 pt from the sides and the bottom, four places with their icons over their words, then the profile
/// (plans/nav-profile.md), the current one in the key blue on a darker inner capsule.
private struct TabBar: View {
    let selection: AppTab
    let select: (AppTab) -> Void
    /// The profile's menu chose a profile with a PIN.
    let pin: (ProfileInfo) -> Void
    let bottomInset: CGFloat
    @Environment(\.dynamicTypeSize) private var type
    @Environment(ProfileSession.self) private var profiles
    @State private var width = CGFloat.infinity

    var body: some View {
        let iconsOnly = tabBarIconsOnly(width: width, type: type)
        HStack(spacing: 0) {
            ForEach(AppTab.bar(kids: profiles.snapshot?.current?.kids == true), id: \.self) { item in
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
            profile(iconsOnly: iconsOnly)
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

    /// The last tab: the profile's avatar over "Profile", lit while Settings shows, opening the profile's menu.
    private func profile(iconsOnly: Bool) -> some View {
        let on = selection == .settings
        return ProfileMenu(onSettings: { select(.settings) }, onPin: pin) { me in
            VStack(spacing: 1) {
                BarAvatar(profile: me, size: 26, lit: on)
                if !iconsOnly {
                    Text(Incognito.tabWord(me))
                        .textFont(11, weight: .semibold, relativeTo: .caption2)
                        .lineLimit(1)
                        .minimumScaleFactor(0.8)
                }
            }
            .foregroundStyle(on ? Theme.Palette.key : .white)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .frame(minWidth: Theme.minTarget, minHeight: Theme.minTarget)
            .background {
                if on { Capsule().fill(Color(white: 0.04).opacity(0.92)) }
            }
            .contentShape(Rectangle())
        }
        // On the menu's own button, as the places carry them on theirs.
        .accessibilityAddTraits(on ? .isSelected : [])
        .accessibilityShowsLargeContentViewer { Text(Incognito.tabWord(profiles.snapshot?.current)) }
    }
}
#endif

#if os(macOS)
/// The window toolbar's only item, top right over the art: Search as a round glass button. The profile ends the
/// ornament at the window's foot, in Settings' old place (plans/nav-profile.md).
private struct TopTools: ToolbarContent {
    let find: () -> Void

    var body: some ToolbarContent {
        // An empty centre: without one the toolbar packs its actions at the left, beside the traffic lights.
        if #available(macOS 26, *) {
            ToolbarItem(placement: .principal) { Color.clear.frame(width: 1, height: 1) }.sharedBackgroundVisibility(.hidden)
            ToolbarItem(placement: .primaryAction) { search }.sharedBackgroundVisibility(.hidden)
        } else {
            ToolbarItem(placement: .principal) { Color.clear.frame(width: 1, height: 1) }
            ToolbarItem(placement: .primaryAction) { search }
        }
    }

    /// The window's edge to the last button's (Search, now the profile is in the ornament): the page's gutter, as
    /// tv.apple.com and the Apple TV app keep their top right on the content's column (the owner, 2026-10-28: flush in the
    /// corner). The toolbar's own margin makes up the last 4.5 pt (measured in the window on macOS 27).
    static let edge = Metrics.regular.gutter - 4.5

    private var search: some View {
        Button(action: find) { Label("Search", glyph: "search", size: 17) }
            .buttonStyle(GlassCircleStyle())
            .help("Search")
            .padding(.trailing, Self.edge)
    }
}
#endif

#if os(tvOS)
/// TV's floating glass pill: the wordmark, the four places, a divider, then
/// Search with its icon and the profile, whose menu opens Settings
/// (plans/nav-profile.md). The current place sits in a lighter inner pill.
private struct TopNav: View {
    let selection: AppTab
    let select: (AppTab) -> Void
    /// Select on the shown place's link: focus goes into the place, as the tvOS tab bar does.
    let enter: () -> Void
    /// The profile menu's Settings: Settings' first page.
    let settings: () -> Void
    /// The profile's menu chose a profile with a PIN.
    let pin: (ProfileInfo) -> Void
    /// Which link has focus: Up from a page enters on the current place, not the nearest link. A link that
    /// keeps it for the settle shows its place, as the tvOS tab bar does; focus stays on it.
    var focused: FocusState<AppTab?>.Binding
    /// The avatar has focus (Menu there goes Home, as on a link).
    var avatar: FocusState<Bool>.Binding
    @Environment(ProfileSession.self) private var profiles

    private var places: [AppTab] { AppTab.bar(kids: profiles.snapshot?.current?.kids == true) }

    var body: some View {
        HStack(spacing: 2) {
            BrandMark(size: Self.mark * 1.5)
                .padding(.leading, Self.mark * 0.6)
                .padding(.trailing, Self.mark * 0.4)
            ForEach(places.filter { !$0.isTool }, id: \.self) { link($0) }
            Capsule().fill(Theme.Palette.lineStrong).frame(width: 1.5, height: Self.mark).padding(.horizontal, 8)
            ForEach(places.filter(\.isTool), id: \.self) { link($0) }
            // Settings has no link: the profile stands for it, lit while it shows.
            // Settings, chosen while it shows, leaves focus on the avatar: the dialog hands focus back to it as it closes.
            ProfileMenu(onSettings: settings, onPin: pin, focus: avatar) {
                BarAvatar(profile: $0, size: Self.mark + 8)
            }
            .buttonStyle(NavLinkStyle(on: selection == .settings))
            .padding(.leading, 6)
        }
        .padding(Self.inset)
        .fixedSize()
        .glass(Capsule(), shadow: true)
        #if os(tvOS)
        .focusSection()
        .defaultFocus(focused, selection.barFocus, priority: .userInitiated)
        .defaultFocus(avatar, selection.barFocus == nil, priority: .userInitiated)
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
            guard let item = focused.wrappedValue, item != now else { return }
            if let link = now.barFocus { focused.wrappedValue = link } else { avatar.wrappedValue = true }
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

    /// Lets art run under the status bar (iOS) and under the Mac's traffic lights,
    /// as the title page's backdrop and Home's hero need; a scrim keeps the status
    /// bar readable once content scrolls under it.
    func fullBleedTop() -> some View {
        #if os(iOS) || os(macOS)
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
/// The page in front also gives the Go menu its Back (Cmd-[): it leaves that page, or asks first
/// when the page holds edits (`leaveAsk`).
private struct BackOnlyWhenShown: ViewModifier {
    @Environment(\.tabShown) private var shown
    @Environment(\.isPresented) private var presented
    @Environment(\.dismiss) private var dismiss
    @Environment(\.leaveAsk) private var leaveAsk
    @Environment(\.sheetOver) private var sheetOver
    @Environment(\.sheetBack) private var sheetBack
    @State private var inFront = false

    func body(content: Content) -> some View {
        content
            .navigationBarBackButtonHidden(backButtonHidden(shown: shown, holdsEdits: leaveAsk != nil))
            .onAppear { inFront = true }
            .onDisappear { inFront = false }
            .focusedSceneValue(\.goBack, goBack)
    }

    private var route: BackRoute { backRoute(presented: presented, sheet: sheetBack) }

    private var goBack: GoBackAction? {
        guard canGoBack(shown: shown, inFront: inFront, presented: !route.isNone, sheetOver: sheetOver) else { return nil }
        return GoBackAction {
            if let leaveAsk {
                leaveAsk()
            } else if case .close(let close) = route {
                close()
            } else {
                dismiss()
            }
        }
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
    /// Cmd-[ (the Go menu's Back) closes this sheet, on the Mac: only the profile picker, which is a place to
    /// leave. A flow with its own steps (onboarding) keeps Back to itself: its pages give the Go menu none.
    var closesOnBack = false
    /// The Mac's sheet takes the whole window (first run's full-bleed still), not the sheet's own small size.
    var fillsWindow = false
    @ViewBuilder let flow: () -> Flow
    /// A place kept alive out of sight presents nothing until it shows again:
    /// on TV it is disabled, and a flow it presented would be disabled too.
    @Environment(\.tabShown) private var shown
    #if os(macOS)
    /// A sheet this one is nested in.
    @Environment(\.sheetOver) private var outerSheetOver
    /// The window's content, title bar included, for a sheet that fills it.
    @State private var window = CGSize.zero
    #endif

    #if os(macOS)
    private var sheetBack: SheetBack { closesOnBack ? .close({ isPresented = false }) : .off }
    #endif

    func body(content: Content) -> some View {
        let presented = Binding { isPresented && shown } set: { isPresented = $0 }
        #if os(macOS)
        // The pages under a sheet give no Back (Cmd-[ would pop one nobody sees), and the sheet's own page closes
        // it. Written on the content, inside the sheet's modifier: the sheet's flow keeps the default.
        content
            .environment(\.sheetOver, sheetCovers(outer: outerSheetOver, own: presented.wrappedValue))
            .onGeometryChange(for: CGSize.self) { CGSize(width: $0.size.width, height: $0.size.height + $0.safeAreaInsets.top) } action: { window = $0 }
            .sheet(isPresented: presented) {
                let size = fillsWindow ? FilmRules.macSheet(window: window) : FilmRules.sheetFloor
                flow()
                    .frame(minWidth: FilmRules.sheetFloor.width, minHeight: FilmRules.sheetFloor.height)
                    .frame(width: fillsWindow ? size.width : nil, height: fillsWindow ? size.height : nil)
                    .environment(\.sheetBack, sheetBack)
            }
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
