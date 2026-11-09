import SwiftUI

/// Title page: art with the decision on top (logo, meta, synopsis, Play and
/// the version under it, library), then credits and the episode contents.
struct TitleScreen: View {
    @State private var model: TitleModel
    let core: CoreClient
    @Environment(\.metrics) private var metrics
    /// Back from a title a kids profile may not open.
    @Environment(\.dismiss) private var dismiss
    @Environment(\.tabShown) private var shown
    /// The place is on screen now: a start that settles later reads it.
    @State private var placeShown = true
    @Environment(SyncModel.self) private var sync
    @State private var width: CGFloat = 0
    @State private var height: CGFloat = 0
    /// The version note and the page's failure under the Mac hero, with their gap.
    @State private var noteHeight: CGFloat = 0
    /// The Mac action row's drawn width: Starring shows only clear of it.
    @State private var actionsWidth = CGFloat.infinity
    /// Content has scrolled under the status bar and the back button.
    @State private var underBar = false
    /// The page's top content inset, nil until measured (0 is a real answer: a window with no status bar).
    @State private var insetTop: CGFloat?
    /// The screen's top safe area (status bar and navigation bar), read outside the full-bleed.
    @State private var safeTop: CGFloat = 0
    /// The window's full height, for the cover (it runs from the screen's top): `fullWindowHeight`.
    @State private var windowHeight: CGFloat = 0
    /// Neither the large cover nor the addon's poster loaded: the backdrop band with the logo.
    @State private var coverFailed = false
    /// TV: the page enters on its primary (Play, Resume or Add a source).
    @FocusState private var primaryFocused: Bool
    /// Versions for the play target: the line under Play, Play itself, and the Versions page.
    @State private var versions: VersionsModel?
    /// Versions for an episode picked from the list, when it is not the play target.
    @State private var episodeVersions: VersionsModel?
    @State private var playing: PlayRoute?
    @State private var showVersions = false
    @State private var showEpisodeVersions = false
    /// MORE: the whole synopsis in a sheet (iOS and macOS).
    @State private var reading = false
    /// The episodes panel beside the info (Mac, iPad) and the Episodes page (iPhone).
    @State private var panelOpen = false
    @State private var showEpisodes = false
    /// The episode a player's Episodes came back for: the panel shows it.
    @State private var episodeShown: String?
    @Environment(\.openTitle) private var parentOpenTitle
    #if !os(tvOS)
    /// What opened the panel, which takes keyboard focus back when it closes.
    @State private var opener: PanelOpener?
    @FocusState private var openerFocus: PanelOpener?
    #endif
    #if !os(tvOS)
    /// The title's own trailer player: its header's muted preview, then Trailer full screen (plans/trailer-inline.md).
    @State private var trailerPlayer = TrailerPlayer()
    @State private var headerTrailer = TrailerState()
    @State private var fullTrailer = false
    /// At least half the hero on screen: the header's trailer plays only then.
    @State private var heroVisible = true
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    #endif
    /// A Calendar link opens the episodes once.
    @State private var openedFromLink = false
    @Environment(\.linkRequests) private var links
    @Environment(\.scenePhase) private var scenePhase
    /// The page is in its navigation stack's front (nothing pushed over it).
    @State private var inFront = false
    @Environment(\.openTab) private var openTab
    @Environment(SearchInput.self) private var searchInput: SearchInput?
    @Environment(ProfileSession.self) private var profiles: ProfileSession?
    #if !os(tvOS)
    /// Downloads (plans/offline-downloads.md 3.2): the keeper's ops, and Versions to choose what to keep.
    @Environment(DownloadKeeper.self) private var keeper: DownloadKeeper?
    @State private var downloadVersions: VersionsModel?
    @State private var showDownloadVersions = false
    /// A season's downloads to delete once its confirmation says yes.
    @State private var deletingSeason: [String]?
    #endif

    init(route: TitleRoute, core: CoreClient) {
        _model = State(initialValue: TitleModel(route: route, core: core))
        self.core = core
    }

    var body: some View {
        page
        .onGeometryChange(for: CGSize.self) { $0.size } action: { (width, height) = ($0.width, $0.height) }
        .onGeometryChange(for: CGFloat.self) {
            fullWindowHeight(viewHeight: $0.size.height, safeTop: $0.safeAreaInsets.top, safeBottom: $0.safeAreaInsets.bottom)
        } action: { windowHeight = $0 }
        .task(id: core.generation) {
            model.start()
            versions?.start()
        }
        .onChange(of: playTarget?.videoId, initial: true) { openVersions() }
        #if DEBUG
        .onChange(of: versions?.snapshot?.recommended?.key) { _, key in
            if DebugHooks.autoPlay, key != nil, playing == nil { playing = versions?.play() }
            if DebugHooks.showVersions, key != nil { showVersions = true }
        }
        // A fixture's pick is there before the view is: play once the view is open.
        .onChange(of: versions?.sub) { _, sub in
            if DebugHooks.autoPlay, sub != nil, DebugHooks.versionsFixture != nil, playing == nil { playing = versions?.play() }
        }
        .onChange(of: versions?.snapshot?.empty?.kind) { _, kind in
            if DebugHooks.showVersions, kind != nil { showVersions = true }
        }
        #endif
        .fullBleedTop()
        // Read outside the full-bleed: inside it the top inset is ignored and reads 0.
        .onGeometryChange(for: CGFloat.self) { $0.safeAreaInsets.top } action: { safeTop = $0 }
        .onChange(of: shown, initial: true) { _, now in placeShown = now }
        // TMDB and Trakt are asked only while the page shows (`titleVisible`): not under the player or
        // Versions, not in another tab, not with the app in the background.
        // Back on the title (from Versions, or a player that ends here): a kind chosen there lists every kind again, so
        // Play plays the core's pick (plans/plugins-and-marketplace.md 4.2).
        .onAppear {
            inFront = true
            versions?.listEveryKind()
            episodeVersions?.listEveryKind()
        }
        .onDisappear { inFront = false }
        .onChange(of: pageVisible, initial: true) { _, now in model.setVisible(now) }
        // At rest too: on a short or wide window the words can start under the bar before any scroll.
        .topScrim(underBar, atRest: true)
        #if os(macOS)
        // With the synopsis sheet open, Cmd-[ closes the sheet instead of popping the title under it (the page
        // still gives Back, so a sheet over the whole shell can switch it off through `sheetOver`).
        .environment(\.leaveAsk, synopsisClose)
        #endif
        .defaultFocus($primaryFocused, true)
        #if os(macOS)
        // Start Over in the Playback menu, for this title only while it is the page in front.
        .focusedSceneValue(\.startOver, startOverAvailable(resume: model.header?.playTarget?.resumeMs, shown: placeShown, covered: playing != nil)
            ? StartOverAction { play(fromStart: true) } : nil)
        #endif
        .navigationDestination(item: $playing) { route in
            PlayerScreen(route: route, versions: [versions, episodeVersions].compactMap { $0 }.first { $0.target.videoId == route.target.videoId }, core: core)
        }
        .navigationDestination(isPresented: $showVersions) {
            if let versions {
                VersionsScreen(model: versions, heading: heading(versions.target), core: core)
            }
        }
        .navigationDestination(isPresented: $showEpisodeVersions) {
            if let episodeVersions {
                VersionsScreen(model: episodeVersions, heading: heading(episodeVersions.target), core: core)
            }
        }
        #if !os(tvOS)
        .navigationDestination(isPresented: $showDownloadVersions) {
            if let downloadVersions {
                VersionsScreen(model: downloadVersions, heading: heading(downloadVersions.target), core: core, keep: keep)
            }
        }
        // "Delete Season N downloads" asks first, as a series and Delete all do (review r1 M5).
        .downloadConfirm(item: $deletingSeason, title: { DownloadCopy.deleteSeries($0.count, details?.name ?? model.route.name) }) {
            keeper?.act(.remove, .ids($0))
        }
        .downloadProblem(keeper)
        #endif
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
        // Both of its players (its own and the one under its Versions) answer Episodes here.
        .environment(\.openTitle, OpenTitleAction { titleOpened($0) })
    }

    /// Episodes from a player over this page (plans/player-bar.md section 5, Apple Episodes): its own title pops back
    /// here and shows the season and episode, with no second copy of the page; any other title goes on the place's way.
    private func titleOpened(_ route: TitleRoute) {
        switch BarRules.episodes(playing: route.id, page: model.route.id) {
        case .selectInPlace:
            (playing, showVersions, showEpisodeVersions) = (nil, false, false)
            if let season = route.season { model.selectSeason(season) }
            episodeShown = route.videoId
            #if !os(tvOS)
            openEpisodes()
            #endif
        case .push:
            parentOpenTitle(route)
        }
    }

    @ViewBuilder private var page: some View {
        #if os(tvOS)
        tvBody
        #else
        v3Body
        #endif
    }

    #if os(tvOS)
    /// Today's page: tvOS keeps it (D9).
    private var tvBody: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.l) {
                hero
                if let failure = model.failure, fallback != .failed(failure) {
                    StateLine(symbol: "triangle-alert", text: failure, spoken: true)
                        .padding(.horizontal, metrics.gutter)
                }
                lower
            }
            .padding(.bottom, Theme.Space.xl)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
    }
    #else
    /// The v3 page on iOS and macOS (plans/design-v3-phase3-apple.md): the full-bleed hero, then today's
    /// credits and episodes until the episodes commit.
    private var v3Body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.l) {
                v3Hero
                VStack(alignment: .leading, spacing: Theme.Space.s) {
                    if !metrics.wide, let details { phoneWords(details) }
                    // A title shown from its downloads with no connection (plans/offline-downloads.md 3.5).
                    if offline {
                        Label(DownloadCopy.titleOffline, glyph: "wifi-off")
                            .textFont(Theme.type.callout, relativeTo: .callout)
                            .foregroundStyle(Theme.Palette.muted)
                    }
                    // Why Resume cannot start, under the hero, never over the art.
                    VersionNote(target: model.header?.playTarget, versions: versions?.snapshot, failure: versions?.failure)
                    if let failure = model.failure, fallback != .failed(failure) {
                        StateLine(symbol: "triangle-alert", text: failure, spoken: true)
                    }
                }
                // Beside the open panel the note keeps to the hero's column.
                .frame(maxWidth: panelOpen && !panelCovers ? WideLayout.panelColumnWidth(window: width) : .infinity, alignment: .leading)
                .padding(.horizontal, metrics.wide ? metrics.gutter : 20)
                // The hero gives the note its room: Season N stays on the first screen (review P3-8).
                .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { noteHeight = $0 > 0 ? $0 + Theme.Space.l : 0 }
                v3Lower
            }
            .padding(.bottom, Theme.Space.xl)
        }
        .scrollIndicators(.hidden)
        // Under the panel's scrim the page is out of reach (it covers the info below about 1020); under the Mac's
        // full-screen trailer too.
        .inert(panelCovers || fullTrailer)
        .background(Theme.Palette.bg)
        .onScrollGeometryChange(for: Bool.self) { $0.contentOffset.y + $0.contentInsets.top > 8 } action: { underBar = $1 }
        // How far the page starts under the screen's top, where a cover's art starts.
        .onScrollGeometryChange(for: CGFloat.self) { $0.contentInsets.top } action: { insetTop = $1 }
        // A scroll view that never reports its inset (an unchanged 0) still shows the cover's words, a moment later.
        .task {
            try? await Task.sleep(for: .milliseconds(150))
            if insetTop == nil { insetTop = 0 }
        }
        #if DEBUG
        .defaultScrollAnchor(DebugHooks.scrollBottom ? .bottom : nil)
        #endif
        .overlay(alignment: .topTrailing) { panelOverlay }
        .navigationDestination(isPresented: $showEpisodes) { episodesPage }
        // A Calendar link names a season: the episodes open on it once the title loads.
        .onChange(of: model.header?.details != nil, initial: true) { _, loaded in
            guard loaded, !openedFromLink, model.route.season != nil else { return }
            openedFromLink = true
            openEpisodes()
        }
        .sheet(isPresented: $reading) {
            SynopsisSheet(title: details?.name ?? model.route.name, text: details?.description ?? "")
        }
        #if os(iOS)
        .fullScreenCover(isPresented: $fullTrailer) { fullTrailerView }
        #else
        .overlay { if fullTrailer { fullTrailerView } }
        #endif
        // The header's trailer: the page in front, the hero half on screen, nothing over it (decision 3).
        .task(id: HeaderTrailerKey(id: details?.trailers.first?.youtubeId, live: pageVisible && heroVisible && !fullTrailer && !offline)) {
            await startHeaderTrailer()
        }
        // Covered (the player, Versions) or gone: its player and web view go too.
        .onChange(of: pageVisible) { _, now in if !now, !fullTrailer { trailerPlayer.release() } }
        .onDisappear { if !fullTrailer { trailerPlayer.release() } }
        // The full screen closed: its web view and content process go too (the header does not replay this visit).
        .onChange(of: fullTrailer) { _, now in if !now { trailerPlayer.release() } }
    }

    private var fullTrailerView: some View {
        FullTrailer(trailers: details?.trailers ?? [], player: trailerPlayer) { fullTrailer = false }
    }

    /// Trailer: full screen in the app; the header's preview stops and does not come back this session.
    private func openTrailer(_ details: TitleDetails) {
        HeaderTrailers.spent.insert(details.id)
        // Stopped here, before the full screen starts on the same player: the header's task never stops that one.
        trailerPlayer.stop()
        fullTrailer = true
    }

    private func startHeaderTrailer() async {
        if headerTrailer.loading {
            trailerPlayer.stop()
            // Scrolled away while it played: cut by the viewer, it does not come back this session (the app in the
            // background is not a cut).
            if !heroVisible, let id = details?.id { HeaderTrailers.spent.insert(id) }
        }
        // Offline the page plays no trailer (plans/offline-downloads.md 4.18).
        guard !fullTrailer, !offline, pageVisible, heroVisible, let details,
              let id = HeroTrailers.checkedId(details.trailers.first?.youtubeId),
              !HeaderTrailers.spent.contains(details.id) else { return }
        try? await Task.sleep(for: .milliseconds(300))
        guard !Task.isCancelled,
              TrailerSignals.now(visible: true, reducedMotion: reduceMotion, hasTrailer: true).allowedLogged() else { return }
        // One player at a time: Home's lets go first.
        TrailerPlayer.shared.release()
        trailerPlayer.play(id, into: headerTrailer) { why in
            if why != "stopped" { HeaderTrailers.spent.insert(details.id) }
        }
    }

    /// The header trailer's sound switch shows while it plays, never in a walk unless the walk asks.
    private var headerSoundShown: Bool {
        #if DEBUG
        headerTrailer.playing && (!DebugHooks.mute || DebugHooks.showSoundSwitch)
        #else
        headerTrailer.playing
        #endif
    }

    private func toggleHeaderSound() {
        TrailerState.shared.sound.toggle()
        trailerPlayer.sound(TrailerState.shared.sound)
    }

    /// The header's trailer under the art, mounted only while one is due and nothing covers it (one web view, one
    /// host: the full-screen trailer takes it).
    @ViewBuilder private var headerSurface: some View {
        if headerTrailer.loading && !fullTrailer { TrailerSurface(player: trailerPlayer) }
    }

    /// The art fades over a playing trailer once YouTube's start controls have gone, its ramps staying.
    private var headerArtOpacity: Double { headerTrailer.shown ? 0 : 1 }

    private var v3HeroHeight: CGFloat {
        if metrics.wide { return WideLayout.titleHeroHeight(width: width, height: height, note: noteHeight) }
        guard width > 0, height > 0 else { return 633 }
        // A cover at its own 2:3, whole, held to 75 percent of the window (covers are vertical, and a
        // backdrop is often a crop of the same art).
        if phoneCover != nil { return coverHeroHeight(width: width, height: coverRoom) }
        // The iPhone's 390x680 hero, its status bar under the art's top, held to most of a short window.
        return min(width * 633 / 390, max(height * 0.85, 280))
    }

    /// The cover the iPhone shows: only where its 2:3 fits (a portrait window, `coverFits`), never on the Mac or iPad.
    private var phoneCover: String? {
        guard !metrics.wide, !coverFailed, width > 0, windowHeight > 0, coverFits(width: width, height: coverRoom) else { return nil }
        return details?.cover
    }

    /// Where a cover starts (R4): under the status bar and the navigation bar's floating Back, the page's top inset,
    /// so Back never sits on the title a cover prints at its top.
    private var coverRow: CGFloat { insetTop ?? 0 }

    /// The window under that row, which a cover's 2:3 is held to (`coverHeroHeight`, `coverFits`).
    private var coverRoom: CGFloat { windowHeight - coverRow }

    @ViewBuilder private var v3Hero: some View {
        if let details {
            let facts = TitleHeroFacts(details, linksGenres: model.header?.linksGenres ?? true)
            Group {
                if metrics.wide {
                    TitleHeroWide(facts: facts, columnWidth: panelOpen && !panelCovers ? WideLayout.panelColumnWidth(window: width) : 900,
                                  open: openLink, more: { reading = true }) { wideActions(details) }
                } else if phoneCover != nil {
                    // The cover stands whole above the words, which start at its foot: with no logo when the core
                    // knows the cover prints the title (metahub's), else with it. Unseen until the page's top inset
                    // is known, so the words never land one inset low and jump.
                    VStack(spacing: 0) {
                        // The page starts at its top inset, where the cover starts too.
                        Color.clear.frame(height: coverHeroHeight(width: width, height: coverRoom))
                        TitleHeroPhone(facts: facts, logo: coverWantsLogo(details)) { phoneButtons() } secondaries: { phoneSecondaries(details) }
                    }
                    .opacity(coverWordsShown(insetTop: insetTop) ? 1 : 0)
                } else {
                    TitleHeroPhone(facts: facts) { phoneButtons() } secondaries: { phoneSecondaries(details) }
                }
            }
            .heroFrame(height: v3HeroHeight, alignment: metrics.wide ? .bottomLeading : .bottom) { EmptyView() }
            .onScrollVisibilityChange(threshold: 0.5) { heroVisible = $0 }
            // The iPhone's header trailer sound, at the art's top trailing corner, where nothing else sits.
            .overlay(alignment: .topTrailing) {
                if !metrics.wide, headerSoundShown {
                    TrailerMute(player: trailerPlayer, size: 44)
                        .padding(.trailing, 16)
                        .padding(.top, safeTop + 4)
                }
            }
            .overlay(alignment: .bottomTrailing) {
                if metrics.wide, !panelOpen {
                    let starring = WideLayout.starringFits(window: width, row: actionsWidth) && !facts.cast.isEmpty
                    VStack(alignment: .trailing, spacing: 16) {
                        // The rated cover above Starring, or above the action row's height (never over it).
                        if let cover = facts.ratedPoster, WideLayout.coverFits(window: width, hero: v3HeroHeight) {
                            RatedCover(url: cover, width: WideLayout.coverWidth)
                        }
                        if starring {
                            HeroStarring(cast: facts.cast)
                        } else {
                            Color.clear.frame(width: 1, height: 40)
                        }
                    }
                    .padding(.trailing, metrics.gutter)
                    .padding(.bottom, HeroLayout.foot)
                }
            }
            .background {
                // The phone's art runs up under the status bar, the Mac's under the traffic lights.
                ZStack(alignment: .top) {
                    if phoneCover != nil {
                        // The iPhone shows the cover first, whole at its 2:3 under the status bar and Back's row
                        // (R4: it prints its title at its top, and Back never sits on it), with no words over it:
                        // only its bottom edge fades into the page. A large cover that does not load gives way to the
                        // addon's poster, then to the name.
                        headerSurface.frame(height: coverHeroHeight(width: width, height: coverRoom)).padding(.top, coverRow)
                        CoverArt(sources: coverArtSources(details), name: details.name, height: coverHeroHeight(width: width, height: coverRoom)) {
                            coverFailed = true
                        }
                        .padding(.top, coverRow)
                        .opacity(headerArtOpacity)
                    } else {
                        headerSurface
                        ArtImage(url: details.background ?? details.poster, maxPixels: ArtSize.backdrop,
                                 alignment: details.background == nil ? .top : .center)
                            .accessibilityHidden(true)
                            .opacity(headerArtOpacity)
                        HeroScrims(wide: metrics.wide, page: Theme.Palette.bg, wash: BackdropStops.titleWash)
                    }
                }
                .ignoresSafeArea(edges: metrics.wide ? [] : .top)
            }
        } else if fallback == .notAllowed {
            StateLine(symbol: "triangle-alert", text: TitleFallback.refused, action: ("Back", { dismiss() }), primary: true, spoken: true)
                .padding(.horizontal, metrics.gutter)
                .padding(.top, failureTop(safeTop: safeTop))
        } else if case .failed(let reason) = fallback {
            StateLine(symbol: "triangle-alert", text: reason, action: ("Try again", model.retry), spoken: true)
                .padding(.horizontal, metrics.gutter)
                // The page runs under the status bar and the floating Back: the line starts below them.
                .padding(.top, failureTop(safeTop: safeTop))
        } else {
            Text("Loading details")
                .textFont(Theme.type.callout, relativeTo: .callout)
                .foregroundStyle(Theme.Palette.muted)
                .frame(maxWidth: .infinity, minHeight: v3HeroHeight)
        }
    }

    /// The page under the hero: the season shelf, Cast, then Information (the Mac's band, the
    /// iPhone's list).
    @ViewBuilder private var v3Lower: some View {
        if let details {
            let seasons = model.header?.seasons ?? []
            VStack(alignment: .leading, spacing: Theme.Space.l) {
                if !seasons.isEmpty {
                    SeasonShelf(seasons: seasons, selected: model.header?.selectedSeason, episodes: model.episodes,
                                minutes: minutes(details.runtime), select: model.selectSeason, actions: episodeActions)
                        .padding(.leading, metrics.wide ? metrics.gutter : 20)
                    // The open panel shows it itself, where the episode was pressed.
                }
                // After the episodes and before Cast; it lands late, under what it follows.
                SimilarRow(tiles: model.similar)
                // A kids profile's Search finds only its list: its names are not links.
                let linked = model.header?.linksGenres ?? true
                if !details.cast.isEmpty {
                    CastShelf(cast: details.cast, open: linked ? { openLink(.person($0)) } : nil).padding(.horizontal, metrics.wide ? metrics.gutter : 20)
                }
                // The writers link as the directors do; on the phone Information lists them.
                if metrics.wide, let writers = details.writers, !writers.isEmpty {
                    ChipGroups(genres: [], directors: [], writers: Array(writers.prefix(3)), linked: linked, open: openLink)
                        .padding(.horizontal, metrics.gutter)
                }
                let rows = informationRows(genres: details.genres, directors: metrics.wide ? details.directors : [], runtime: details.runtime, seasons: seasons,
                                           selectedSeason: model.header?.selectedSeason, episodes: model.episodes.count)
                if metrics.wide {
                    InfoBand(synopsis: details.description, genres: details.genres, rows: rows)
                } else {
                    // The phone's rated cover sits in the body above Information, wide enough for the band's digits.
                    if let cover = details.ratedPoster {
                        RatedCover(url: cover, width: 200).frame(maxWidth: .infinity)
                    }
                    InfoList(genres: Array(details.genres.prefix(3)), directors: details.directors, writers: details.writers ?? [], rows: rows,
                             linked: linked, open: openLink)
                        .padding(.horizontal, 20)
                }
            }
        }
    }

    private var episodeActions: EpisodeActionsSet {
        EpisodeActionsSet(
            play: { play($0) }, chooseVersion: chooseVersion,
            setWatched: { model.setWatched($0, $1); sync.schedule() }, allEpisodes: { openEpisodes(from: $0) }, focus: $openerFocus,
            resumable: (model.header?.playTarget?.resumeMs ?? 0) > 0 ? model.header?.playTarget?.videoId : nil,
            startOver: { play(fromStart: true) }, downloads: episodeDownloads)
    }

    private func openEpisodes(from: PanelOpener? = nil) {
        opener = from
        if metrics.wide { panelOpen = true } else { showEpisodes = true }
    }

    /// The panel covers the info on a narrow window: the page dims and goes inert (WideLayout).
    private var panelCovers: Bool { WideLayout.pageInert(wide: metrics.wide, panelOpen: panelOpen, window: width) }

    /// Closes the panel and gives keyboard focus back to what opened it.
    private func closePanel() {
        panelOpen = false
        let back = opener
        Task { @MainActor in openerFocus = back }
    }

    /// The Mac's and iPad's panel, pinned under the header 40 from the right, as tall as the window
    /// less 48, over a scrim only when it covers the info.
    @ViewBuilder private var panelOverlay: some View {
        if metrics.wide, panelOpen {
            ZStack(alignment: .topTrailing) {
                if panelCovers {
                    Color.black.opacity(0.5)
                        .contentShape(Rectangle())
                        .onTapGesture { closePanel() }
                        .accessibilityHidden(true)
                }
                episodePanel(close: closePanel)
                    .padding(16)
                    .frame(width: 420)
                    .frame(maxHeight: .infinity, alignment: .top)
                    .background(Theme.Palette.surface, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
                    .padding(.vertical, 24)
                    .padding(.trailing, 40)
                    .accessibilityAddTraits(panelCovers ? .isModal : [])
            }
        }
    }

    /// The iPhone's Episodes page.
    private var episodesPage: some View {
        episodePanel(close: nil)
            .padding(.horizontal, 20)
            .padding(.top, Theme.Space.s)
            .background(Theme.Palette.bg)
            .navigationTitle("Episodes")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.large)
            #endif
    }

    private func episodePanel(close: (() -> Void)?) -> some View {
        EpisodePanel(
            seasons: model.header?.seasons ?? [], selected: model.header?.selectedSeason, episodes: model.episodes,
            minutes: minutes(details?.runtime), select: model.selectSeason, actions: episodeActions,
            scrollTo: episodeShown ?? model.route.videoId, close: close)
    }

    /// The Mac's action row: the primary, Trailer, the Library and Watched capsule, Versions.
    private func wideActions(_ details: TitleDetails) -> some View {
        let shown = Incognito.trailerShown(hasTrailers: !details.trailers.isEmpty, offline: offline, incognito: Incognito.on)
        let trailer: (() -> Void)? = shown ? { openTrailer(details) } : nil
        let sound: (() -> Void)? = headerSoundShown ? { toggleHeaderSound() } : nil
        return WideActions(
            trailer: trailer,
            marks: titleMarks(
                kind: details.kind, inLibrary: model.header?.inLibrary ?? false, watched: model.header?.watched ?? false,
                librarySettable: model.header?.canSetLibrary == true),
            toggle: { toggle($0, details) }, versionsReady: versions != nil, showVersions: { showVersions = true },
            sound: sound, soundOn: TrailerState.shared.sound, versionsShown: !offline
        ) { playBlock(details) } download: { titleDownload(details) }
        // Starring keeps clear of the row as drawn (review P2-1).
        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { actionsWidth = $0 }
    }

    /// The iPhone's primary, 178 wide with the time left under it, and Apple's Library control.
    private func phoneButtons() -> some View {
        HStack(alignment: .top, spacing: 12) {
            VStack(spacing: 6) {
                if let details { playBlock(details) }
                if let target = model.header?.playTarget, let resume = target.resumeMs, resume > 0 {
                    Text(resumeText(resumeMs: resume, leftMs: target.leftMs, runtime: details?.runtime))
                        .textFont(13, relativeTo: .footnote)
                        .dynamicTypeSize(...DynamicTypeSize.xxxLarge)
                        .foregroundStyle(Theme.Palette.muted)
                }
            }
            // No Library on a kids profile: its Library is the owner's list.
            if model.header?.canSetLibrary == true {
                LibraryControl(on: model.header?.inLibrary ?? false) {
                    model.setInLibrary(!(model.header?.inLibrary ?? false))
                    sync.schedule()
                }
            }
            if let details, details.kind == "movie" {
                let watched = model.header?.watched ?? false
                WatchedControl(on: watched) { toggle(.watched(on: watched), details) }
            }
        }
    }

    /// Trailer and Versions: secondaries with their words, wrapping to a second centered line when they do
    /// not fit. Watched sits beside Library, a small round toggle.
    private func phoneSecondaries(_ details: TitleDetails) -> some View {
        FlowRow(spacing: 8, centered: true) {
            // Start over in sight, first, never only in Resume's long press (the Mac and iPad draw it beside Resume).
            if (model.header?.playTarget?.resumeMs ?? 0) > 0 {
                Button { play(fromStart: true) } label: { Label("Start over", glyph: "rotate-ccw") }
                    .buttonStyle(PillButtonStyle())
                    .disabled(PlayBlock.inert(failure: versions?.failure, versions: versions?.snapshot))
            }
            if Incognito.trailerShown(hasTrailers: !details.trailers.isEmpty, offline: offline, incognito: Incognito.on) {
                Button { openTrailer(details) } label: { Label("Trailer", glyph: "film") }
                    .buttonStyle(PillButtonStyle())
            }
            if !offline {
                Button { showVersions = true } label: { Label("Versions", glyph: "layers") }
                    .buttonStyle(PillButtonStyle())
                    .disabled(versions == nil)
            }
            // After Versions, never in the panel rows (plans/offline-downloads.md 3.2).
            titleDownload(details)
        }
    }

    /// The iPhone's synopsis with MORE and its meta row, under the hero on the page.
    private func phoneWords(_ details: TitleDetails) -> some View {
        let facts = TitleHeroFacts(details)
        return VStack(alignment: .leading, spacing: 8) {
            if let synopsis = facts.synopsis { HeroSynopsis(text: synopsis, lines: 2, more: { reading = true }) }
            MetaLine(ratings: [], parts: facts.meta, size: 13, imdbLink: facts.imdbLink, marks: facts.scores)
                .foregroundStyle(Theme.Palette.muted)
        }
    }

    private func playBlock(_ details: TitleDetails) -> some View {
        PlayBlock(
            target: model.header?.playTarget, nextAirs: model.header?.nextAirs, hasEpisodes: !model.episodes.isEmpty,
            runtime: details.runtime, versions: versions?.snapshot, failure: versions?.failure, wide: metrics.wide,
            play: { play(fromStart: $0) }, focus: $primaryFocused, v3: true)
    }

    private func toggle(_ mark: TitleMark, _ details: TitleDetails) {
        switch mark {
        case .library(let on): model.setInLibrary(!on)
        case .watched(let on): model.setWatched(details.id, !on)
        }
        sync.schedule()
    }

    /// A linked chip: a genre opens Discover on the title's kind, a director opens Search.
    private func openLink(_ link: TitleLink) {
        switch link {
        case .genre(let genre):
            links?.discover = DiscoverRequest(kind: details?.kind ?? "movie", genre: genre)
            openTab(.discover, link: true)
        case .person(let name):
            searchInput?.link(name)
            openTab(.search, link: true)
        }
    }

    /// A title shown from its downloads with no connection: no Trailer and no Versions.
    private var offline: Bool { model.header?.offline == true }

    private var kids: Bool { profiles?.snapshot?.current?.kids == true }

    /// The control and its items: not on a kids profile, whose download ops the core refuses.
    private var downloadsHere: Bool { keeper != nil && DownloadGate.controls(tv: DownloadGate.tv, kids: kids) }

    /// The title's Download: a movie's control, or a series' menu of its seasons; on a kids profile only
    /// "Downloaded" as a plain badge.
    @ViewBuilder private func titleDownload(_ details: TitleDetails) -> some View {
        let header = model.header
        let series = !(header?.seasons.isEmpty ?? true)
        if downloadsHere, let keeper {
            if series {
                let season = header?.selectedSeason
                DownloadControlButton(
                    control: DownloadCopy.seriesControl(header?.seriesDownload), look: .pill,
                    entries: seriesEntries(details, keeper: keeper),
                    start: { if let season { keeper.add(kind: details.kind, itemId: details.id, scope: "season", season: season) } },
                    note: Incognito.on ? Incognito.downloadNote : nil)
            } else {
                let badge = header?.download
                DownloadControlButton(
                    control: DownloadCopy.control(badge), look: .pill,
                    entries: badge.map { videoEntries($0, keeper: keeper, play: { play(fromStart: false) }, choose: { chooseDownload(videoId: details.id) }) } ?? [],
                    start: { keeper.add(kind: details.kind, itemId: details.id, scope: "video", videoId: details.id) },
                    choose: { chooseDownload(videoId: details.id) },
                    note: Incognito.on ? Incognito.downloadNote : nil)
            }
        } else if keeper != nil, kids, series ? (header?.seriesDownload?.downloaded ?? 0) > 0 : header?.download?.state == .done {
            DownloadedBadge()
        }
    }

    /// A video's items as menu entries: the ops go to the core, Play and the versions go somewhere.
    private func videoEntries(_ badge: DownloadBadge, keeper: DownloadKeeper, play: @escaping () -> Void, choose: @escaping () -> Void) -> [DownloadMenuEntry] {
        DownloadCopy.control(badge).items.map { item in
            DownloadMenuEntry(id: item.rawValue, label: item.label, glyph: item.glyph) {
                switch item {
                case .play: play()
                case .another, .choose: choose()
                default: _ = keeper.run(item, id: badge.id)
                }
            }
        }
    }

    /// "Download Season 2 (8 episodes)", "Download every season (3 seasons, 28 episodes)", the
    /// season's state and its delete.
    private func seriesEntries(_ details: TitleDetails, keeper: DownloadKeeper) -> [DownloadMenuEntry] {
        let header = model.header
        let season = header?.selectedSeason
        return DownloadCopy.seriesMenu(season: season, group: header?.seasonDownload, series: header?.seriesDownload).map { item in
            let run: (() -> Void)? = switch item.act {
            case .downloadSeason: { if let season { keeper.add(kind: details.kind, itemId: details.id, scope: "season", season: season) } }
            case .downloadSeries: { keeper.add(kind: details.kind, itemId: details.id, scope: "series") }
            case .deleteSeason: {
                let ids = model.episodes.filter { $0.season == season }.compactMap { $0.download?.id }
                if !ids.isEmpty {
                    if DownloadCopy.asksFirst(item.act) { deletingSeason = ids } else { keeper.act(.remove, .ids(ids)) }
                }
            }
            case .status: nil
            }
            return DownloadMenuEntry(id: item.label, label: item.label, glyph: item.act == .deleteSeason ? DownloadItem.delete.glyph : item.act == .status ? nil : DownloadItem.start.glyph, run: run)
        }
    }

    /// The episodes' downloads, for the cards, the panel rows and their menus.
    private var episodeDownloads: EpisodeDownloads? {
        guard downloadsHere, let keeper, let details else { return nil }
        return EpisodeDownloads(
            entries: { episode in
                episode.download.map { videoEntries($0, keeper: keeper, play: { play(episode) }, choose: { chooseDownload(videoId: episode.videoId) }) } ?? []
            },
            start: { keeper.add(kind: details.kind, itemId: details.id, scope: "video", videoId: $0.videoId) },
            choose: { chooseDownload(videoId: $0.videoId) },
            note: Incognito.on ? Incognito.downloadNote : nil)
    }

    /// "Choose a version to download": Versions ranked to keep on this device.
    private func chooseDownload(videoId: String) {
        guard let details else { return }
        let model = VersionsModel(
            target: withFacts(PlayTargetInfo(
                kind: details.kind, itemId: details.id, videoId: videoId, title: details.name, poster: details.poster, resumeMs: nil), details),
            core: core, purpose: "download")
        model.start()
        downloadVersions = model
        showDownloadVersions = true
    }

    /// A version picked to keep: added with that row, then back to the title.
    private func keep(_ key: String) {
        guard let keeper, let model = downloadVersions, let sub = model.sub else { return }
        keeper.add(kind: model.target.kind, itemId: model.target.itemId, scope: "video", videoId: model.target.videoId, pick: (sub, key))
        showDownloadVersions = false
    }
    #endif

    private var details: TitleDetails? { model.header?.details }
    #if os(macOS)
    private var synopsisClose: (() -> Void)? {
        guard reading else { return nil }
        return { reading = false }
    }
    #endif
    private var fallback: TitleFallback { titleFallback(header: model.header?.state, failure: model.failure) }

    private var pageVisible: Bool {
        titlePageShows(inFront: inFront, placed: placeShown, active: scenePhase == .active,
                       covered: playing != nil || showVersions || showEpisodeVersions)
    }

    private var playTarget: PlayTargetInfo? {
        guard let details, let target = model.header?.playTarget else { return nil }
        return withFacts(PlayTargetInfo(
            kind: details.kind, itemId: details.id, videoId: target.videoId, title: details.name,
            poster: details.poster, resumeMs: target.resumeMs), details)
    }

    /// The runtime and years the title page has, and the episode's date and
    /// season size, for the ranking (plans/m13-stream-choice.md 4.3).
    private func withFacts(_ target: PlayTargetInfo, _ details: TitleDetails) -> PlayTargetInfo {
        var target = target
        target.runtime = details.runtime
        target.releaseInfo = details.releaseInfo
        if let episode = model.episodes.first(where: { $0.videoId == target.videoId }) {
            target.released = episode.released
            target.episodesInSeason = model.episodes.filter { $0.season == episode.season }.count
        }
        return target
    }

    /// Play as the versions stand (`TitlePlay`): the pick, the source kept here, or the player at once
    /// while versions are found, which starts the pick once it settles; else Versions, which holds why
    /// nothing plays with its way on; else nothing (Play is inert).
    private func play(fromStart: Bool) {
        guard let versions else { return }
        switch TitlePlay.of(versions.snapshot) {
        case .player(let key):
            var route = key.flatMap { versions.play($0) } ?? versions.pending(fromStart: fromStart)
            route.fromStart = fromStart
            playing = route
        case .versions: showVersions = true
        case .inert: break
        }
    }

    /// One Versions view per play target. Replacing the model closes the old
    /// view, unless a player still holds it.
    private func openVersions() {
        guard let target = playTarget else { versions = nil; return }
        let model = VersionsModel(target: target, core: core)
        model.start()
        versions = model
    }

    /// "S2:E3 What Is Dead May Never Die" or the movie's name.
    private func heading(_ target: PlayTargetInfo) -> String {
        guard let season = target.season, let number = target.episode else {
            return details?.name ?? model.route.name
        }
        let title = model.episodes.first { $0.videoId == target.videoId }?.title
        return ["S\(season):E\(number)", title].compactMap { $0 }.joined(separator: " ")
    }

    /// The open Versions view for an episode, reusing the play target's.
    private func versions(for episode: Episode) -> VersionsModel? {
        guard let details else { return nil }
        if let versions, versions.target.videoId == episode.videoId { return versions }
        if let episodeVersions, episodeVersions.target.videoId == episode.videoId { return episodeVersions }
        let model = VersionsModel(
            target: withFacts(PlayTargetInfo(
                kind: details.kind, itemId: details.id, videoId: episode.videoId, title: details.name,
                poster: details.poster, resumeMs: nil), details),
            core: core)
        model.start()
        episodeVersions = model
        return model
    }

    /// Plays an episode from the list as Play does: the player opens at once and starts its pick, or
    /// Versions holds why nothing plays.
    private func play(_ episode: Episode) {
        guard let model = versions(for: episode) else { return }
        switch TitlePlay.of(model.snapshot) {
        case .player(let key): playing = key.flatMap { model.play($0) } ?? model.pending()
        case .versions: chooseVersion(episode)
        // Back alone would be a dead end: nothing opens, as the title's inert Play.
        case .inert: break
        }
    }

    private func chooseVersion(_ episode: Episode) {
        guard let model = versions(for: episode) else { return }
        if model === versions { showVersions = true } else { showEpisodeVersions = true }
    }

    /// The art runs under the bar at rest; the page scrim takes over once the
    /// title's first words come this close to the top, so they never pass
    /// bare under the status bar or the Dynamic Island (up to about 68 pt),
    /// whatever the art's height and the text size.
    nonisolated static let wordsClearance: CGFloat = 88

    private var heroHeight: CGFloat {
        #if os(tvOS)
        return 640
        #else
        guard width > 0 else { return wideHero ? 520 : 300 }
        // Held to under half the window: a landscape phone shows the title and Play
        // above the tab bar, not only art. Portrait phones keep their width rule.
        let window = height > 0 ? max(height * 0.4, 160) : .infinity
        return wideHero ? min(max(width * 0.46, 420), 600) : min(width * 0.78, window)
        #endif
    }

    private var wideHero: Bool {
        #if os(tvOS)
        return true
        #else
        return WideLayout.titleOverArt(wide: metrics.wide, window: height)
        #endif
    }

    #if os(tvOS)
    @ViewBuilder private var hero: some View {
        if wideHero {
            ZStack(alignment: .bottomLeading) {
                Backdrop(url: details?.background, poster: details?.poster, height: heroHeight, wide: true)
                decision.frame(maxWidth: 580, alignment: .leading)
                    .padding(.horizontal, metrics.gutter)
                    .padding(.bottom, Theme.Space.m)
            }
        } else {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                if let details, details.background == nil {
                    // No backdrop: no empty band on a phone; the page starts at the
                    // title, clear of the status bar and the back button.
                    logo.padding(.horizontal, metrics.gutter).padding(.top, 116)
                } else {
                    ZStack(alignment: .bottomLeading) {
                        Backdrop(url: details?.background, poster: nil, height: heroHeight, wide: false)
                        logo.padding(.horizontal, metrics.gutter)
                    }
                }
                decisionBody.padding(.horizontal, metrics.gutter)
            }
        }
    }

    #endif

    private var logo: some View {
        TitleLogo(
            logo: details?.logo, name: details?.name ?? model.route.name,
            maxWidth: wideHero ? 460 : 280, height: wideHero ? 120 : 96)
            // The first words on the page: where they are decides the scrim.
            .onGeometryChange(for: Bool.self) { $0.frame(in: .scrollView).minY < Self.wordsClearance } action: { underBar = $1 }
    }

    private var decision: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            logo
            decisionBody
        }
    }

    @ViewBuilder private var decisionBody: some View {
        VStack(alignment: .leading, spacing: 14) {
            if let details {
                MetaLine(
                    ratings: ratingParts(details.ratings), parts: metaParts(details, seasons: model.header?.seasons ?? []),
                    size: Theme.type.callout,
                    imdbLink: details.ratings?.imdb == nil ? nil : details.imdbUrl.flatMap(URL.init(string:)))
                    .foregroundStyle(Theme.Palette.muted)
                if let description = details.description {
                    Synopsis(text: description, lines: 3)
                }
                PlayBlock(
                    target: model.header?.playTarget, nextAirs: model.header?.nextAirs, hasEpisodes: !model.episodes.isEmpty,
                    runtime: details.runtime, versions: versions?.snapshot, failure: versions?.failure, wide: metrics.wide,
                    play: { play(fromStart: $0) }, focus: $primaryFocused)
                actions(details)
            } else if fallback == .notAllowed {
                StateLine(symbol: "triangle-alert", text: TitleFallback.refused, action: ("Back", { dismiss() }), primary: true, spoken: true)
            } else if case .failed(let reason) = fallback {
                StateLine(symbol: "triangle-alert", text: reason, action: ("Try again", model.retry), spoken: true)
            } else {
                Text("Loading details")
                    .textFont(Theme.type.callout, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.muted)
            }
        }
    }

    /// Versions, Trailer (when the metadata names one), Library and, for a
    /// movie, Watched: round buttons with their words, a held state lit. A
    /// phone gives them one row of equal columns (a word may take two lines),
    /// never ragged wrapped rows.
    private func actions(_ details: TitleDetails) -> some View {
        let header = model.header
        let inLibrary = header?.inLibrary ?? false
        let watched = header?.watched ?? false
        // No Library on a kids profile: its Library is the owner's list.
        let settable = header?.canSetLibrary == true
        let count = versions?.snapshot.map { $0.rows.count + ($0.recommended == nil ? 0 : 1) } ?? 0
        let layout = metrics.wide ? AnyLayout(FlowRow(spacing: 26)) : AnyLayout(HStackLayout(alignment: .top, spacing: 8))
        let column = metrics.wide ? nil : CGFloat.infinity
        return layout {
            Button { showVersions = true } label: {
                Label(count > 0 ? "Versions (\(count))" : "Versions", glyph: "layers")
            }
            .buttonStyle(RoundButtonStyle())
            .disabled(versions == nil)
            .frame(maxWidth: column)
            #if os(tvOS)
            if settable {
                Button { model.setInLibrary(!inLibrary); sync.schedule() } label: {
                    Label(inLibrary ? "In Library" : "Add to Library", glyph: inLibrary ? "bookmark-check" : "bookmark-plus")
                }
                .buttonStyle(RoundButtonStyle(on: inLibrary))
                .frame(maxWidth: column)
                // A state, spoken as one: "In Library, toggle, on".
                .accessibilityLabel("In Library")
                .accessibilityAddTraits(.isToggle)
                .accessibilityValue(inLibrary ? "On" : "Off")
            }
            #else
            if metrics.wide {
                // The desktop frame's paired capsule: Library, and Watched for a movie.
                let marks = titleMarks(kind: details.kind, inLibrary: inLibrary, watched: watched, librarySettable: settable)
                if !marks.isEmpty {
                    MarkCapsule(marks: marks) { mark in
                        switch mark {
                        case .library(let on): model.setInLibrary(!on)
                        case .watched(let on): model.setWatched(details.id, !on)
                        }
                        sync.schedule()
                    }
                }
            } else if settable {
                LibraryControl(on: inLibrary) { model.setInLibrary(!inLibrary); sync.schedule() }
            }
            #endif
            if details.kind == "movie" && !capsuleHoldsWatched {
                Button { model.setWatched(details.id, !watched); sync.schedule() } label: {
                    Label(watched ? "Watched" : "Mark as watched", glyph: "circle-check")
                }
                .buttonStyle(RoundButtonStyle(on: watched))
                .frame(maxWidth: column)
                .accessibilityLabel("Watched")
                .accessibilityAddTraits(.isToggle)
                .accessibilityValue(watched ? "On" : "Off")
            }
        }
        .padding(.top, 4)
    }

    /// The Mac's capsule carries Watched; elsewhere it stays a round button until the v3 hero.
    private var capsuleHoldsWatched: Bool {
        #if os(tvOS)
        return false
        #else
        return metrics.wide
        #endif
    }

    #if os(tvOS)
    @ViewBuilder private var lower: some View {
        if let details {
            let series = !(model.header?.seasons.isEmpty ?? true)
            if metrics.wide && series && (width == 0 || WideLayout.creditsBesideEpisodes(page: width - 2 * metrics.gutter)) {
                HStack(alignment: .top, spacing: Theme.Space.xl) {
                    Credits(details: details, series: true).frame(width: 360, alignment: .leading).modifier(ReadableOnTV())
                    episodes
                }
                .padding(.horizontal, metrics.gutter)
            } else {
                // Phone: what to watch first, then who made it.
                VStack(alignment: .leading, spacing: Theme.Space.l) {
                    if series { episodes }
                    Credits(details: details, series: series).modifier(ReadableOnTV())
                }
                .frame(maxWidth: 760, alignment: .leading)
                .padding(.horizontal, metrics.gutter)
            }
        }
    }

    private var episodes: some View {
        EpisodeList(
            seasons: model.header?.seasons ?? [],
            selected: model.header?.selectedSeason,
            episodes: model.episodes,
            current: model.header?.playTarget?.videoId,
            runtime: details?.runtime,
            select: model.selectSeason,
            play: { play($0) },
            chooseVersion: chooseVersion,
            setWatched: { model.setWatched($0, $1); sync.schedule() })
    }
    #endif
}

/// Three lines and More; the whole text once opened.
private struct Synopsis: View {
    let text: String
    let lines: Int
    @State private var open = false
    /// Heights of the text as shown and in full: More only when something is cut.
    @State private var shown: CGFloat = 0
    @State private var full: CGFloat = 0

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(text)
                .textFont(Theme.type.body)
                .foregroundStyle(Theme.Palette.muted)
                .lineLimit(open ? nil : lines)
                .fixedSize(horizontal: false, vertical: true)
                .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { shown = $0 }
                .background(alignment: .topLeading) {
                    Text(text)
                        .textFont(Theme.type.body)
                        .fixedSize(horizontal: false, vertical: true)
                        .hidden()
                        .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { full = $0 }
                }
            if !open, full > shown + 1 {
                Button("More") {
                    #if os(tvOS)
                    // A remote scrolls only by moving focus: the whole text opens as a reader.
                    reading = true
                    #else
                    withAnimation(Theme.Motion.panel) { open = true }
                    #endif
                }
                .buttonStyle(.textLink)
                .textFont(Theme.type.callout, weight: .semibold, relativeTo: .callout)
                .foregroundStyle(Theme.Palette.text)
                .accessibilityHint("Shows the whole synopsis")
            }
        }
        #if os(tvOS)
        .fullScreenCover(isPresented: $reading) { SynopsisReader(text: text) }
        #endif
    }

    #if os(tvOS)
    @State private var reading = false
    #endif
}

#if os(tvOS)
/// The whole synopsis on TV, read to its end: blocks of whole sentences that
/// take focus in turn, so Down scrolls the page a block at a time; Menu
/// returns to the title.
private struct SynopsisReader: View {
    let text: String
    @FocusState private var focused: Int?

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                ForEach(Array(readingBlocks(text).enumerated()), id: \.offset) { index, block in
                    Text(block)
                        .textFont(Theme.type.body)
                        .foregroundStyle(focused == index ? Theme.Palette.text : Theme.Palette.muted)
                        .fixedSize(horizontal: false, vertical: true)
                        .padding(Theme.Space.s)
                        .focusable()
                        .focused($focused, equals: index)
                        .modifier(FocusRing(shape: RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous), active: focused == index))
                }
            }
            .frame(maxWidth: 1100, alignment: .leading)
            .frame(maxWidth: .infinity)
            .padding(Theme.Space.xl)
        }
        .background(Theme.Palette.bg)
        .defaultFocus($focused, 0)
    }
}
#endif

#if os(tvOS)
/// Backdrop art faded into the page; with no backdrop, the poster blurred at
/// 35% over the page colour, so the right side is never a blank band.
private struct Backdrop: View {
    let url: String?
    let poster: String?
    let height: CGFloat
    let wide: Bool

    var body: some View {
        let bg = Theme.Palette.bg
        art
            .frame(maxWidth: .infinity)
            .frame(height: height)
            .clipped()
            .overlay {
                if wide {
                    GeometryReader { geo in
                        LinearGradient(stops: BackdropStops.gradient(BackdropStops.titleSide(width: geo.size.width), bg), startPoint: .leading, endPoint: .trailing)
                    }
                }
                LinearGradient(stops: BackdropStops.gradient(BackdropStops.titleFoot(wide: wide), bg), startPoint: .top, endPoint: .bottom)
            }
            .accessibilityHidden(true)
    }

    @ViewBuilder private var art: some View {
        if url == nil, let poster {
            Theme.Palette.bg.overlay {
                ArtImage(url: poster, maxPixels: ArtSize.poster)
                    .blur(radius: 40)
                    .opacity(0.35)
            }
        } else {
            ArtImage(url: url, maxPixels: ArtSize.backdrop)
        }
    }
}

#endif

#if os(tvOS)
/// The reserved warning-only line under Play: the core's line for why nothing plays, else nothing. No
/// addon's name: it shows on
/// the Addons pages alone (design/DECISION.md, Shelves).
struct VersionLine: View {
    let versions: VersionsSnapshot?

    var body: some View {
        if let line = titleWarning(versions) {
            Label(line, glyph: "circle-alert")
                .foregroundStyle(Theme.Palette.muted)
        }
    }
}
#endif

#if !os(tvOS)
/// Why Resume cannot start, under the v3 hero: the versions did not load, or the core's line for why
/// none can play here. The finding and found states show nothing, as the frames draw
/// (plans/design-v3-phase3-apple.md 2.1).
private struct VersionNote: View {
    let target: PlayTarget?
    let versions: VersionsSnapshot?
    let failure: String?

    var body: some View {
        Group {
            if let failure {
                Label("Versions could not load: \(failure)", glyph: "triangle-alert")
            } else if target != nil, let line = titleWarning(versions) {
                Label(line, glyph: "circle-alert")
            }
        }
        .textFont(Theme.type.callout, relativeTo: .callout)
        .foregroundStyle(Theme.Palette.muted)
        .fixedSize(horizontal: false, vertical: true)
    }
}
#endif

#if !os(tvOS)
/// Titles whose header trailer ended, failed or was opened full screen this session: it does not play again.
@MainActor
enum HeaderTrailers {
    static var spent: Set<String> = []
}

private struct HeaderTrailerKey: Equatable {
    let id: String?
    let live: Bool
}
#endif
