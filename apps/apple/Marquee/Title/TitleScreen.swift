import SwiftUI

/// Title page: art with the decision on top (logo, meta, synopsis, Play and
/// the version under it, library), then credits and the episode contents.
struct TitleScreen: View {
    @State private var model: TitleModel
    let core: CoreClient
    @Environment(\.metrics) private var metrics
    @Environment(\.openURL) private var openURL
    @Environment(SyncModel.self) private var sync
    @State private var width: CGFloat = 0
    @State private var height: CGFloat = 0
    /// Content has scrolled under the status bar and the back button.
    @State private var underBar = false
    /// TV: the page enters on its primary (Play, Resume or Add a source).
    @FocusState private var primaryFocused: Bool
    /// Versions for the play target: the line under Play, Play itself, and the Versions page.
    @State private var versions: VersionsModel?
    /// Versions for an episode picked from the list, when it is not the play target.
    @State private var episodeVersions: VersionsModel?
    @State private var playing: PlayRoute?
    @State private var showVersions = false
    @State private var showEpisodeVersions = false
    @State private var showAddons = false
    /// The episode whose version is being found, and why the last one could not start.
    @State private var starting: String?
    @State private var startFailure: EpisodeFailure?

    init(route: TitleRoute, core: CoreClient) {
        _model = State(initialValue: TitleModel(route: route, core: core))
        self.core = core
    }

    var body: some View {
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
        #endif
        .fullBleedTop()
        // At rest too: on a short or wide window the words can start under the bar before any scroll.
        .topScrim(underBar, atRest: true)
        .defaultFocus($primaryFocused, true)
        .navigationDestination(item: $playing) { route in
            PlayerScreen(route: route, versions: [versions, episodeVersions].compactMap { $0 }.first { $0.sub == route.versionsSub }, core: core)
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
        .navigationDestination(isPresented: $showAddons) { AddonsPage(core: core, sourcesOnly: true) }
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
    }

    private var details: TitleDetails? { model.header?.details }

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

    /// Plays an episode from the list: its pick once it settles, as Play does.
    private func play(_ episode: Episode) async {
        let tapped = ContinuousClock.now
        guard let model = versions(for: episode) else { return }
        (starting, startFailure) = (episode.videoId, nil)
        let route = await model.settledPick(tapped: tapped)
        guard starting == episode.videoId else { return }
        starting = nil
        if let route {
            playing = route
        } else {
            let text = model.snapshot.flatMap { $0.done ? VersionWords.none($0) : nil }
            startFailure = EpisodeFailure(videoId: episode.videoId, text: text ?? "No version answered yet. Choose a version to keep waiting.", noVersion: text != nil)
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
                MetaLine(rating: details.rating, parts: metaParts(details, seasons: model.header?.seasons ?? []), size: Theme.type.callout)
                    .foregroundStyle(Theme.Palette.muted)
                if let description = details.description {
                    Synopsis(text: description, lines: 3)
                }
                PlayBlock(
                    target: model.header?.playTarget, runtime: details.runtime, versions: versions?.snapshot,
                    failure: versions?.failure, wide: metrics.wide,
                    play: {
                        if let route = versions?.play() {
                            playing = route
                        } else {
                            // Sources still answering: play the pick once it settles.
                            Task { playing = await versions?.settledPick() }
                        }
                    },
                    addSource: { showAddons = true }, focus: $primaryFocused)
                actions(details)
            } else if case .failed(let reason) = model.header?.state {
                StateLine(symbol: "triangle-alert", text: reason, action: ("Try again", model.retry))
            } else {
                Text("Loading details")
                    .font(.text(Theme.type.callout, relativeTo: .callout))
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
            Button { model.setInLibrary(!inLibrary); sync.schedule() } label: {
                Label(inLibrary ? "In Library" : "Add to Library", glyph: inLibrary ? "bookmark-check" : "bookmark-plus")
            }
            .buttonStyle(RoundButtonStyle(on: inLibrary))
            .frame(maxWidth: column)
            // A state, spoken as one: "In Library, toggle, on".
            .accessibilityLabel("In Library")
            .accessibilityAddTraits(.isToggle)
            .accessibilityValue(inLibrary ? "On" : "Off")
            if details.kind == "movie" {
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
            starting: starting,
            failure: startFailure,
            select: model.selectSeason,
            play: { episode in Task { await play(episode) } },
            chooseVersion: chooseVersion,
            addSource: { showAddons = true },
            setWatched: { model.setWatched($0, $1); sync.schedule() })
    }
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
                .font(.text(Theme.type.body))
                .foregroundStyle(Theme.Palette.muted)
                .lineLimit(open ? nil : lines)
                .fixedSize(horizontal: false, vertical: true)
                .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { shown = $0 }
                .background(alignment: .topLeading) {
                    Text(text)
                        .font(.text(Theme.type.body))
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
                .font(.text(Theme.type.callout, weight: .semibold, relativeTo: .callout))
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
                        .font(.text(Theme.type.body))
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
                    LinearGradient(
                        stops: [.init(color: bg.opacity(0.95), location: 0), .init(color: bg.opacity(0.6), location: 0.42), .init(color: .clear, location: 0.75)],
                        startPoint: .leading, endPoint: .trailing)
                }
                LinearGradient(
                    stops: [.init(color: bg.opacity(wide ? 0 : 0.6), location: 0), .init(color: .clear, location: 0.3), .init(color: bg.opacity(0.85), location: 0.82), .init(color: bg, location: 1)],
                    startPoint: .top, endPoint: .bottom)
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

/// Play or Resume with the resume point, and the chosen version under it.
/// Never a Play that can only fail: with no version at all the primary
/// becomes Add a source; while sources still answer, Play waits for the pick.
private struct PlayBlock: View {
    let target: PlayTarget?
    /// The title's runtime ("57 min"), so Resume says the time left.
    let runtime: String?
    let versions: VersionsSnapshot?
    let failure: String?
    let wide: Bool
    let play: () -> Void
    let addSource: () -> Void
    /// TV: the page enters on this block's primary.
    var focus: FocusState<Bool>.Binding

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            if versions?.done == true, versions?.recommended == nil {
                Button(action: addSource) {
                    Label("Add a source", glyph: "plus")
                        .frame(minWidth: wide ? 160 : nil, maxWidth: wide ? nil : .infinity)
                }
                .buttonStyle(PillButtonStyle(prominent: true))
                .focused(focus)
            } else {
                Button(action: play) {
                    HStack(spacing: 10) {
                        Glyph("play-fill")
                        Text(target?.label ?? "Play")
                        if let resume = target?.resumeMs, resume > 0 {
                            Text(resumeText(resumeMs: resume, leftMs: target?.leftMs, runtime: runtime)).fontWeight(Font.Weight.regular.legible).opacity(0.8)
                        }
                    }
                    .frame(minWidth: wide ? 160 : nil, maxWidth: wide ? nil : .infinity)
                }
                .buttonStyle(PillButtonStyle(prominent: true))
                .focused(focus)
                // The reason shows right under it.
                .disabled(failure != nil)
            }
            Group {
                if let failure {
                    Label("Versions could not load: \(failure)", glyph: "triangle-alert")
                        .foregroundStyle(Theme.Palette.muted)
                } else {
                    VersionLine(versions: versions)
                }
            }
            // Wraps whole on a small phone or a large text size: the status and source decide the pick.
            .font(.text(Theme.type.callout, relativeTo: .callout))
            .fixedSize(horizontal: false, vertical: true)
        }
    }
}

/// "720p  472 MB  (dot) Torrent, 153 seeds  via Torrentio", or why there is none yet.
private struct VersionLine: View {
    let versions: VersionsSnapshot?

    var body: some View {
        if let best = versions?.recommended {
            let dot = Text("\u{25CF}").font(.system(size: 9)).foregroundStyle(VersionWords.tone(best.status).color)
            let via = Text("via \(best.via)").foregroundStyle(Theme.Palette.muted)
            Text("\(VersionWords.line(best))   \(dot) \(VersionWords.status(best.status))   \(via)")
                .foregroundStyle(Theme.Palette.text)
                .accessibilityLabel("\(VersionWords.line(best)), \(VersionWords.status(best.status)), via \(best.via)")
        } else if let versions, versions.done {
            Label(VersionWords.none(versions), glyph: "circle-alert")
                .foregroundStyle(Theme.Palette.muted)
        } else {
            HStack(spacing: 8) {
                ProgressView().controlSize(.mini)
                Text("Finding versions")
            }
            .foregroundStyle(Theme.Palette.muted)
            .accessibilityElement(children: .combine)
        }
    }
}
