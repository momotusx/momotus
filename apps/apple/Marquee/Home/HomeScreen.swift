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
    /// The shell's word: a pushed title or the player is over Home.
    @Environment(\.heroCovered) private var covered
    @Environment(SyncModel.self) private var sync
    @Environment(ProfileSession.self) private var profiles
    @State private var width: CGFloat = 0
    @State private var height: CGFloat = 0
    @State private var onboarding = false
    /// The account card's Sign in and second-way-in nudge.
    @State private var showAccount = false
    /// Rows have scrolled under the status bar.
    @State private var underBar = false
    @State private var position = ScrollPosition(edge: .top)
    /// The hero's Versions, held while its player plays.
    @State private var heroVersions: VersionsModel?
    @State private var playing: PlayRoute?
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
                // A deletion, the account gone, a new device: first, above the hero (the owner, 2026-11-02). Nothing
                // when there is none, so the hero keeps its place.
                AccountCard(top: true) { showAccount = true }
                    .padding(.horizontal, metrics.gutter)
                    // Clear of the Mac's toolbar and the TV's tabs, as the setup line is; the phone's list starts under its bar.
                    .padding(.top, metrics.wide ? Theme.Space.xxl : 0)
                // Without catalogs, or with none answering, there is no art to show.
                if !model.needsSetup && !model.allFailed {
                    // The profile is the bar's (plans/nav-profile.md): the tab bar's last tab, the Mac's toolbar.
                    hero
                        #if !os(tvOS)
                        // The trailer's sound switch at the hero's bottom trailing corner, level with the dots, on
                        // every screen that plays trailers (the owner, 2026-10-26; plans/appletv-polish.md 18). From
                        // the moment the trailer plays, so a sound kept from earlier is never heard without its
                        // switch (review r4, W2).
                        .overlay(alignment: .bottomTrailing) {
                            if TrailerState.shared.playing && !walkMuted {
                                TrailerMute()
                                    .padding(.trailing, metrics.gutter)
                                    .padding(.bottom, HeroFoot.soundSwitch(wide: metrics.wide))
                            }
                        }
                        #endif
                        // The first shelf follows the hero's foot as the frames have it: 12 pt on a wide
                        // screen, none on the phone, where the ramp has reached the black page.
                        .padding(.bottom, (metrics.wide ? 12 : 0) - metrics.rowSpacing)
                }
                // The other account cards, by priority (plans/accounts.md 4.3): the nudge, "Signed in from".
                AccountCard { showAccount = true }
                    .padding(.horizontal, metrics.gutter)
                content
            }
            .padding(.bottom, Theme.Space.xl)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .onScrollGeometryChange(for: Bool.self) { $0.contentOffset.y + $0.contentInsets.top > 0 } action: { underBar = $1 }
        #if !os(iOS)
        // TV Back takes a scrolled Home to its top; the Mac's ornament darkens once the art has left it.
        .onScrollGeometryChange(for: Bool.self) { $0.contentOffset.y + $0.contentInsets.top > 8 } action: { scrolled.wrappedValue = $1 }
        #endif
        #if os(macOS)
        // The hero's art runs to the window's top, under the traffic lights (variant 4: no strip).
        .fullBleedTop()
        #if DEBUG
        .scrollPosition($position)
        .task {
            guard DebugHooks.homeScrollY > 0 else { return }
            try? await Task.sleep(for: .seconds(6))
            position.scrollTo(y: DebugHooks.homeScrollY)
        }
        .task {
            guard DebugHooks.scrollWalk else { return }
            try? await Task.sleep(for: .seconds(12))
            await ScrollWalk.run()
        }
        #endif
        #endif
        #if os(tvOS)
        .scrollPosition($position)
        .onChange(of: topRequest) {
            withAnimation(Theme.Motion.panel) { position.scrollTo(edge: .top) }
            resetFocus(in: startScope)
        }
        // The hero's Play comes from the core a moment after its page. The start focus, on Details meanwhile, stays
        // there when Play lands: a focus that moved then would sit under a viewer's quick press. (Web and Android
        // hold the first focus until Play lands instead; the focus engine here always has one.)
        .focusScope(startScope)
        #endif
        .topScrim(underBar || model.needsSetup || model.allFailed)
        .onGeometryChange(for: CGSize.self) { $0.size } action: { (width, height) = ($0.width, $0.height) }
        #if DEBUG
        .onAppear { if DebugHooks.takeOnboarding() { onboarding = true } }
        .task {
            guard DebugHooks.retryRowsAfter > 0 else { return }
            try? await Task.sleep(for: .seconds(DebugHooks.retryRowsAfter))
            for row in model.rows { if case .failed = row.state { model.retry(row) } }
        }
        #endif
        .navigationDestination(isPresented: $showAccount) { SyncScreen() }
        .navigationDestination(item: $playing) { route in
            PlayerScreen(route: route, versions: heroVersions, core: core)
        }
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
        .onAppear {
            if profiles.resumeOnboarding { onboarding = true }
            // Back on Home from the hero's Versions or player: every kind is listed again (plans/plugins-and-marketplace.md 4.2).
            heroVersions?.listEveryKind()
        }
        .onChange(of: onboarding) { _, open in profiles.resumeOnboarding = open }
        .modifier(FullScreen(isPresented: $onboarding, fillsWindow: true) {
            NavigationStack {
                OnboardingScreen(core: core) { onboarding = false }
            }
            .environment(\.metrics, metrics)
            .environment(sync)
            .environment(profiles)
            .environment(core)
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
        case .ready where model.allFailed:
            couldNotLoad.padding(.horizontal, metrics.gutter)
        case .ready:
            // The card sits under Continue Watching, or first without it.
            if !model.rows.contains(where: { $0.rowId == continueRow }) { offerCard }
            ForEach(model.rows, id: \.rowId) { row in
                RowView(row: row, retry: { model.retry(row) }, startScope: start == .row(row.rowId) ? startScopeID : nil, removal: model.removal)
                if row.rowId == continueRow { offerCard }
            }
            if model.needsSetup { setup.padding(.horizontal, metrics.gutter) }
        }
    }

    /// The hero's Play, as the title page's: the player opens at once and starts the pick, or
    /// Versions holds why nothing plays.
    private func playHero(_ hero: Hero) {
        guard let play = hero.play else { return }
        let tile = hero.tile
        let target = PlayTargetInfo(kind: tile.kind, itemId: tile.id, videoId: play.videoId, title: tile.name, poster: tile.plainPoster, resumeMs: play.resumeMs)
        let versions = heroVersions?.target == target ? heroVersions! : VersionsModel(target: target, core: core)
        versions.start()
        heroVersions = versions
        switch TitlePlay.of(versions.snapshot) {
        case .player(let key): playing = key.flatMap { versions.play($0) } ?? versions.pending()
        case .versions: showHeroVersions = true
        // Back alone would be a dead end: nothing opens, as the title's inert Play.
        case .inert: break
        }
    }

    /// The carousel on the phone and the Mac; tvOS keeps its single hero, so its start focus and Menu-back stay
    /// as they are (plans/appletv-polish.md 4.1).
    @ViewBuilder private var hero: some View {
        #if os(tvOS)
        HeroView(hero: model.header.hero, height: heroHeight, startScope: start == .hero ? startScopeID : nil, play: playHero)
        #else
        if model.header.heroes.isEmpty {
            HeroView(hero: nil, height: heroHeight, play: playHero)
        } else {
            HeroCarousel(heroes: model.header.heroes, height: heroHeight, play: playHero)
                // Home's own pages over the hero cover it too: the account, the hero's Versions and
                // player, the recommended addons, first run (review r2, R2-4).
                .environment(\.heroCovered, covered || showAccount || showHeroVersions || playing != nil || reviewing || onboarding)
        }
        #endif
    }

    /// A walk build never draws the sound switch (`-MQMute YES`), unless `-MQShowSoundSwitch YES` asks to see it; the
    /// player stays silent either way (`TrailerPlayer.sound`).
    private var walkMuted: Bool {
        #if DEBUG
        DebugHooks.mute && !DebugHooks.showSoundSwitch
        #else
        false
        #endif
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
                text: "None of the addons you have on shows a catalog here. Set up Momotus offers the recommended addons, or add an addon yourself.",
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
            // tv.apple.com's 16:9 hero from the window's top, held so the first shelf's title shows
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
                Button(action: setUp) { Label("Set up Momotus", glyph: "wrench") }
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
/// how many, Review (the addons step, which names them) and Not now.
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
            Text(DefaultsRules.cardLine(names.count))
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
