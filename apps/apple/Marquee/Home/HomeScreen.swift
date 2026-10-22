import SwiftUI

/// Home: hero, then Continue Watching and catalog rows in core order.
struct HomeScreen: View {
    let model: HomeModel
    /// Bumped by the shell (TV Back) to bring Home back to its top row.
    var topRequest = 0
    /// Home is scrolled off its top row.
    var scrolled: Binding<Bool> = .constant(false)
    @Environment(CoreClient.self) private var core
    @Environment(\.metrics) private var metrics
    @Environment(SyncModel.self) private var sync
    @Environment(\.tabShown) private var shown
    /// Home is on screen now: a hero start that settles later reads it.
    @State private var placeShown = true
    @Environment(ProfileSession.self) private var profiles
    @State private var width: CGFloat = 0
    @State private var height: CGFloat = 0
    @State private var onboarding = false
    /// The account card's Sign in and second-way-in nudge.
    @State private var showAccount = false
    @State private var showProfiles = false
    /// Rows have scrolled under the status bar.
    @State private var underBar = false
    @State private var position = ScrollPosition(edge: .top)
    /// The hero's Versions while its Play settles, held while it plays.
    @State private var heroVersions: VersionsModel?
    @State private var starting = false
    @State private var playing: PlayRoute?
    @State private var showSources = false
    @State private var showHeroVersions = false
    @State private var reviewing = false
    #if os(tvOS)
    /// The hero's Play is where focus goes back to.
    @Namespace private var startScope
    @Environment(\.resetFocus) private var resetFocus
    #endif

    var body: some View {
        ScrollView(.vertical) {
            LazyVStack(alignment: .leading, spacing: metrics.rowSpacing) {
                // Without catalogs, or with none answering, there is no art to show.
                if !model.needsSetup && !model.allFailed {
                    HeroView(hero: model.header.hero, height: heroHeight, startScope: start == .hero ? startScopeID : nil, play: playHero, starting: starting)
                        // Phone and tablet: the profile button, with Sign in another device
                        // in its menu (4.2), sits at the header's top right.
                        .overlay(alignment: .topTrailing) {
                            if showsProfileButton { ProfileButton(signInAnother: signInAnother) { showProfiles = true }.padding(.trailing, 14).padding(.top, 5) }
                        }
                        // The first shelf follows the hero's foot as the frames have it: 12 pt on a wide
                        // screen, none on the phone, where the ramp has reached the black page.
                        .padding(.bottom, (metrics.wide ? 12 : 0) - metrics.rowSpacing)
                }
                // One account card, by priority (plans/accounts.md 4.3).
                AccountCard { showAccount = true }
                    .padding(.horizontal, metrics.gutter)
                content
            }
            .padding(.bottom, Theme.Space.xl)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .onScrollGeometryChange(for: Bool.self) { $0.contentOffset.y + $0.contentInsets.top > 0 } action: { underBar = $1 }
        #if os(tvOS)
        .scrollPosition($position)
        .onScrollGeometryChange(for: Bool.self) { $0.contentOffset.y + $0.contentInsets.top > 8 } action: { scrolled.wrappedValue = $1 }
        .onChange(of: topRequest) {
            withAnimation(Theme.Motion.panel) { position.scrollTo(edge: .top) }
            resetFocus(in: startScope)
        }
        .focusScope(startScope)
        #endif
        .topScrim(underBar || model.needsSetup || model.allFailed)
        .onGeometryChange(for: CGSize.self) { $0.size } action: { (width, height) = ($0.width, $0.height) }
        .onChange(of: shown, initial: true) { _, now in placeShown = now }
        #if DEBUG
        .onAppear { if DebugHooks.takeOnboarding() { onboarding = true } }
        .task {
            guard DebugHooks.retryRowsAfter > 0 else { return }
            try? await Task.sleep(for: .seconds(DebugHooks.retryRowsAfter))
            for row in model.rows { if case .failed = row.state { model.retry(row) } }
        }
        #endif
        .navigationDestination(isPresented: $showProfiles) { ProfilePicker(mode: .push, core: core) }
        .navigationDestination(isPresented: $showAccount) { SyncScreen() }
        .navigationDestination(item: $playing) { route in
            PlayerScreen(route: route, versions: heroVersions, core: core)
        }
        .navigationDestination(isPresented: $showSources) { AddonsPage(core: core, sourcesOnly: true) }
        .navigationDestination(isPresented: $reviewing) {
            RecommendedAddonsPage(core: core, scope: .missing, hasCatalog: model.offer.addons.snapshot?.addons.contains { $0.enabled && $0.provides.contains("catalog") } == true)
        }
        .task(id: model.offer.addons.taskId) { model.offer.start() }
        .task(id: model.offer.addons.snapshot) { await model.offer.refresh() }
        // First run: the recommended addons before anything else.
        .onChange(of: model.offer.fresh, initial: true) { _, fresh in if fresh { onboarding = true } }
        .navigationDestination(isPresented: $showHeroVersions) {
            if let heroVersions {
                VersionsScreen(model: heroVersions, heading: heroVersions.target.title, core: core)
            }
        }
        // A restarted core builds Home again: onboarding opens again if it was showing.
        .onAppear { if profiles.resumeOnboarding { onboarding = true } }
        .onChange(of: onboarding) { _, open in profiles.resumeOnboarding = open }
        .modifier(FullScreen(isPresented: $onboarding) {
            NavigationStack {
                OnboardingScreen(core: core) { onboarding = false }
            }
            .environment(\.metrics, metrics)
            .environment(sync)
            .environment(profiles)
            .environment(core)
        })
    }

    /// iPhone and iPad; a Mac and a TV carry it in their navigation.
    private var showsProfileButton: Bool {
        #if os(iOS)
        true
        #else
        !metrics.wide
        #endif
    }

    /// The profile menu's Sign in another device: a phone or tablet in an account,
    /// never a kids profile (a Mac approves nothing).
    private var signInAnother: (() -> Void)? {
        #if os(iOS)
        sync.joined && profiles.snapshot?.current?.kids != true ? { AppRouter.shared.requestScan() } : nil
        #else
        nil
        #endif
    }

    @ViewBuilder private var content: some View {
        switch model.phase {
        case .opening:
            // The core may still be opening: the shell shows at once, rows to come.
            ForEach(0..<3, id: \.self) { _ in SkeletonRow() }
        case .failed(let message):
            StateLine(symbol: "triangle-alert", text: message, action: ("Try again", model.reload))
                .padding(.horizontal, metrics.gutter)
        case .ready where model.allFailed:
            couldNotLoad.padding(.horizontal, metrics.gutter)
        case .ready:
            // The card sits under Continue Watching, or first without it.
            if !model.rows.contains(where: { $0.rowId == continueRow }) { offerCard }
            ForEach(model.rows, id: \.rowId) { row in
                RowView(row: row, retry: { model.retry(row) }, startScope: start == .row(row.rowId) ? startScopeID : nil)
                if row.rowId == continueRow { offerCard }
            }
            if model.needsSetup { setup.padding(.horizontal, metrics.gutter) }
        }
    }

    /// The hero's Play, as the title page's: its pick once it settles; with
    /// no version at all the Add a source step; still waiting, its Versions.
    private func playHero(_ tile: Tile) {
        guard !starting, let videoId = tile.playVideoId else { return }
        let tapped = ContinuousClock.now
        let target = PlayTargetInfo(kind: tile.kind, itemId: tile.id, videoId: videoId, title: tile.name, poster: tile.poster, resumeMs: nil)
        let versions = heroVersions?.target == target ? heroVersions! : VersionsModel(target: target, core: core)
        heroVersions = versions
        starting = true
        Task {
            let route = await versions.settledPick(tapped: tapped)
            starting = false
            switch SettledStart.of(picked: route != nil, done: versions.snapshot?.done == true, placeShown: placeShown) {
            case .play: playing = route
            case .addSource: showSources = true
            case .versions: showHeroVersions = true
            case .nothing: break
            }
        }
    }

    private var continueRow: String { "continue-watching" }

    /// "New recommended addons", once per profile and list version.
    @ViewBuilder private var offerCard: some View {
        // An empty Home offers the same addons through Set up Marquee.
        if !model.needsSetup, let names = model.offer.names(kids: profiles.snapshot?.current?.kids == true) {
            RecommendedCard(
                names: names,
                review: {
                    Task { await model.offer.seen() }
                    reviewing = true
                },
                notNow: { Task { await model.offer.seen() } })
            .padding(.horizontal, metrics.gutter)
        }
    }

    /// Every lane ended with nothing and one failed: one state, one primary.
    private var couldNotLoad: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            if !metrics.wide {
                Wordmark(size: 22).padding(.bottom, Theme.Space.l)
            }
            Text("Home could not load")
                .display(Theme.type.title1)
                .foregroundStyle(Theme.Palette.text)
                .accessibilityAddTraits(.isHeader)
            Text("None of your addons answered. Check the network, then try again.")
                .textFont(Theme.type.body)
                .foregroundStyle(Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
            Button("Try again", action: model.reload)
                .buttonStyle(PillButtonStyle(prominent: true))
                .padding(.top, 4)
        }
        .padding(.top, metrics.wide ? Theme.Space.xxl : Theme.Space.xxl - 20)
    }

    /// No rows: no addon that is on has a catalog, or every row is hidden.
    private var setup: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            if !metrics.wide {
                Wordmark(size: 22).padding(.bottom, Theme.Space.l)
            }
            CatalogSetup(
                title: "Home fills up from your addons",
                text: "None of the addons you have on shows a catalog here. Set up Marquee offers the recommended addons, or add an addon yourself.",
                core: core, setUp: { onboarding = true })
        }
        .padding(.top, metrics.wide ? Theme.Space.xxl : Theme.Space.xxl - 20)
    }

    /// Where TV focus enters and Menu returns: Continue Watching when it leads, else the hero, else the first row with tiles.
    private var start: HomeStart? {
        HomeStart.target(rows: model.rows, hero: model.header.hero != nil)
    }

    private var startScopeID: Namespace.ID? {
        #if os(tvOS)
        startScope
        #else
        nil
        #endif
    }

    private var heroHeight: CGFloat {
        #if os(tvOS)
        return 600
        #else
        guard width > 0, height > 0 else { return metrics.wide ? 506 : 633 }
        if metrics.wide {
            // tv.apple.com's 16:9 hero under the black header, held so the first shelf's title shows
            // on the first screen; never under 400, the glass check's floor.
            return max(min(width * 0.5625, height - 90), 400)
        }
        // The iPhone's 390x680 hero, its 47 pt status bar under the art's top, held to most of a
        // short window (a landscape phone).
        return min(width * 633 / 390, max(height * 0.85, 280))
        #endif
    }
}

/// No catalog to show (empty Home and Discover): why, then Set up Marquee,
/// whose Start fresh offers the recommended addons with no typing on a
/// remote, and Add an addon. On TV the focus enters on Set up Marquee.
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
                .textFont(Theme.type.body)
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

/// The card that offers recommended addons a profile does not have yet:
/// their names, Review (the addons step for them) and Not now.
private struct RecommendedCard: View {
    let names: [String]
    let review: () -> Void
    let notNow: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("New recommended addons")
                .display(Theme.type.title3)
                .foregroundStyle(Theme.Palette.text)
                .accessibilityAddTraits(.isHeader)
            Text(DefaultsRules.namesLine(names))
                .textFont(Theme.type.callout, relativeTo: .callout)
                .foregroundStyle(Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
            // Centered on one line: the words beside the pill sit on its middle.
            HStack(spacing: 12) {
                Button("Review", action: review).buttonStyle(PillButtonStyle())
                Button("Not now", action: notNow)
                    .buttonStyle(.textLink)
                    .textFont(Theme.type.callout, weight: .semibold, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.text)
            }
            .padding(.top, 4)
        }
        .padding(Theme.Space.s + 4)
        .frame(maxWidth: 760, alignment: .leading)
        // A solid surface with no edge: content, not glass (design/DECISION.md, Surfaces).
        .background(Theme.Palette.surface, in: RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
    }
}
