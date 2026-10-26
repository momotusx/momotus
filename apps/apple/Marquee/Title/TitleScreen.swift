import SwiftUI

/// Title page: art with the decision on top (logo, meta, synopsis, Play and
/// the version under it, library), then credits and the episode contents.
struct TitleScreen: View {
    @State private var model: TitleModel
    let core: CoreClient
    @Environment(\.metrics) private var metrics
    @Environment(\.openURL) private var openURL
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
    @State private var insetTop: CGFloat = 0
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
    #if !os(tvOS)
    /// What opened the panel, which takes keyboard focus back when it closes.
    @State private var opener: PanelOpener?
    @FocusState private var openerFocus: PanelOpener?
    #endif
    /// A Calendar link opens the episodes once.
    @State private var openedFromLink = false
    @Environment(\.linkRequests) private var links
    @Environment(\.scenePhase) private var scenePhase
    /// The page is in its navigation stack's front (nothing pushed over it).
    @State private var inFront = false
    @Environment(\.openTab) private var openTab
    @Environment(SearchInput.self) private var searchInput: SearchInput?

    init(route: TitleRoute, core: CoreClient) {
        _model = State(initialValue: TitleModel(route: route, core: core))
        self.core = core
    }

    var body: some View {
        page
        .onGeometryChange(for: CGSize.self) { $0.size } action: { (width, height) = ($0.width, $0.height) }
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
        .onChange(of: shown, initial: true) { _, now in placeShown = now }
        // TMDB and Trakt are asked only while the page shows (`titleVisible`): not under the player or
        // Versions, not in another tab, not with the app in the background.
        .onAppear { inFront = true }
        .onDisappear { inFront = false }
        .onChange(of: pageVisible, initial: true) { _, now in model.setVisible(now) }
        // At rest too: on a short or wide window the words can start under the bar before any scroll.
        .topScrim(underBar, atRest: true)
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
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
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
                if let failure = model.failure {
                    StateLine(symbol: "triangle-alert", text: failure)
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
                    // Why Resume cannot start, under the hero, never over the art.
                    VersionNote(target: model.header?.playTarget, versions: versions?.snapshot, failure: versions?.failure)
                    if let failure = model.failure {
                        StateLine(symbol: "triangle-alert", text: failure)
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
        // Under the panel's scrim the page is out of reach (it covers the info below about 1020).
        .inert(panelCovers)
        .background(Theme.Palette.bg)
        .onScrollGeometryChange(for: Bool.self) { $0.contentOffset.y + $0.contentInsets.top > 8 } action: { underBar = $1 }
        // How far the page starts under the screen's top, where a cover's art starts.
        .onScrollGeometryChange(for: CGFloat.self) { $0.contentInsets.top } action: { insetTop = $1 }
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
    }

    private var v3HeroHeight: CGFloat {
        if metrics.wide { return WideLayout.titleHeroHeight(width: width, height: height, note: noteHeight) }
        guard width > 0, height > 0 else { return 633 }
        // A cover at its own 2:3, whole, held to 75 percent of the window (covers are vertical, and a
        // backdrop is often a crop of the same art).
        if phoneCover != nil { return coverHeroHeight(width: width, height: height) }
        // The iPhone's 390x680 hero, its status bar under the art's top, held to most of a short window.
        return min(width * 633 / 390, max(height * 0.85, 280))
    }

    /// The cover the iPhone shows: only where its 2:3 fits (a portrait window, `coverFits`), never on the Mac or iPad.
    private var phoneCover: String? {
        guard !metrics.wide, width > 0, height > 0, coverFits(width: width, height: height) else { return nil }
        return details?.cover
    }

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
                        Color.clear.frame(height: max(0, coverHeroHeight(width: width, height: height) - insetTop))
                        TitleHeroPhone(facts: facts, logo: coverWantsLogo(details)) { phoneButtons() } secondaries: { phoneSecondaries(details) }
                    }
                    .opacity(insetTop > 0 ? 1 : 0)
                } else {
                    TitleHeroPhone(facts: facts) { phoneButtons() } secondaries: { phoneSecondaries(details) }
                }
            }
            .heroFrame(height: v3HeroHeight, alignment: metrics.wide ? .bottomLeading : .bottom) { EmptyView() }
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
                // The phone's art runs up under the status bar; the Mac's starts under the black header.
                ZStack(alignment: .top) {
                    if let cover = phoneCover {
                        // The iPhone shows the cover first, whole at its 2:3 from the screen's top, with no words
                        // over it: the status bar's wash at its top, and only its bottom edge fading into the page.
                        // A large cover that does not load gives way to the addon's poster, then to the name.
                        CoverArt(url: cover, fallbackURL: details.poster, name: details.name, height: coverHeroHeight(width: width, height: height))
                    } else {
                        ArtImage(url: details.background ?? details.poster, maxPixels: ArtSize.backdrop,
                                 alignment: details.background == nil ? .top : .center)
                            .accessibilityHidden(true)
                        HeroScrims(wide: metrics.wide, page: Theme.Palette.bg, wash: BackdropStops.titleWash)
                    }
                }
                .ignoresSafeArea(edges: metrics.wide ? [] : .top)
            }
        } else if case .failed(let reason) = model.header?.state {
            StateLine(symbol: "triangle-alert", text: reason, action: ("Try again", model.retry))
                .padding(.horizontal, metrics.gutter)
                .padding(.top, Theme.Space.xl)
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
            startOver: { play(fromStart: true) })
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
            scrollTo: model.route.videoId, close: close)
    }

    /// The Mac's action row: the primary, Trailer, the Library and Watched capsule, Versions.
    private func wideActions(_ details: TitleDetails) -> some View {
        WideActions(
            trailer: details.trailers.first.flatMap { URL(string: $0.url) },
            marks: titleMarks(
                kind: details.kind, inLibrary: model.header?.inLibrary ?? false, watched: model.header?.watched ?? false,
                librarySettable: model.header?.canSetLibrary == true),
            toggle: { toggle($0, details) }, versionsReady: versions != nil, showVersions: { showVersions = true }
        ) { playBlock(details) }
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
            if let trailer = details.trailers.first, let url = URL(string: trailer.url) {
                Button { openURL(url) } label: { Label("Trailer", glyph: "film") }
                    .buttonStyle(PillButtonStyle())
            }
            Button { showVersions = true } label: { Label("Versions", glyph: "layers") }
                .buttonStyle(PillButtonStyle())
                .disabled(versions == nil)
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
    #endif

    private var details: TitleDetails? { model.header?.details }

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
            } else if case .failed(let reason) = model.header?.state {
                StateLine(symbol: "triangle-alert", text: reason, action: ("Try again", model.retry))
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
            // Not on TV: it has no browser, and a YouTube link may open nothing.
            #if !os(tvOS)
            if let trailer = details.trailers.first, let url = URL(string: trailer.url) {
                Button { openURL(url) } label: { Label("Trailer", glyph: "film") }
                    .buttonStyle(RoundButtonStyle())
                    .frame(maxWidth: column)
            }
            #endif
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
