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
    @Environment(ProfileSession.self) private var profiles
    @State private var width: CGFloat = 0
    @State private var height: CGFloat = 0
    @State private var onboarding = false
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
        #if os(tvOS)
        .scrollPosition($position)
        .onScrollGeometryChange(for: Bool.self) { $0.contentOffset.y + $0.contentInsets.top > 8 } action: { scrolled.wrappedValue = $1 }
        .onChange(of: topRequest) {
            withAnimation(Theme.Motion.panel) { position.scrollTo(edge: .top) }
            resetFocus(in: startScope)
        }
        .focusScope(startScope)
        #endif
        #if os(macOS)
        // The card runs up under the window toolbar, where the navigation pill floats over it.
        .fullBleedTop()
        #endif
        .topScrim(underBar || model.needsSetup || model.allFailed)
        .onGeometryChange(for: CGSize.self) { $0.size } action: { (width, height) = ($0.width, $0.height) }
        #if DEBUG
        .onAppear { if DebugHooks.takeOnboarding() { onboarding = true } }
        #endif
        .navigationDestination(isPresented: $showProfiles) { ProfilePicker(mode: .push, core: core) }
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
            if let route {
                playing = route
            } else if versions.snapshot?.done == true {
                showSources = true
            } else {
                showHeroVersions = true
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
                .font(.text(Theme.type.body))
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
        guard width > 0, height > 0 else { return metrics.wide ? 460 : 520 }
        // Held to 60% of the window, so Continue Watching shows on an SE and Play in landscape.
        let window = height * 0.6
        return metrics.wide ? min(max(width * 0.36, 400), 560, max(window, 300)) : min(520, max(window, 280))
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
                .font(.text(Theme.type.callout, relativeTo: .callout))
                .foregroundStyle(Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
            // Centered on one line: the words beside the pill sit on its middle.
            HStack(spacing: 12) {
                Button("Review", action: review).buttonStyle(PillButtonStyle())
                Button("Not now", action: notNow)
                    .buttonStyle(.textLink)
                    .font(.text(Theme.type.callout, weight: .semibold, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.text)
            }
            .padding(.top, 4)
        }
        .padding(Theme.Space.s + 4)
        .frame(maxWidth: 760, alignment: .leading)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
    }
}
