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
    /// Content has scrolled under the status bar and the back button.
    @State private var underBar = false
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
        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
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
        .onScrollGeometryChange(for: Bool.self) { $0.contentOffset.y + $0.contentInsets.top > scrimAfter } action: { underBar = $1 }
        .fullBleedTop()
        .topScrim(underBar)
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
        return PlayTargetInfo(
            kind: details.kind, itemId: details.id, videoId: target.videoId, title: details.name,
            poster: details.poster, resumeMs: target.resumeMs)
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
            target: PlayTargetInfo(
                kind: details.kind, itemId: details.id, videoId: episode.videoId, title: details.name,
                poster: details.poster, resumeMs: nil),
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

    /// The art runs under the bar at rest; past it, the page scrim takes over.
    private var scrimAfter: CGFloat {
        if !metrics.wide, details != nil, details?.background == nil { return 24 }
        return max(24, heroHeight - 160)
    }

    private var heroHeight: CGFloat {
        #if os(tvOS)
        return 640
        #else
        guard width > 0 else { return metrics.wide ? 520 : 300 }
        return metrics.wide ? min(max(width * 0.46, 420), 600) : width * 0.78
        #endif
    }

    @ViewBuilder private var hero: some View {
        if metrics.wide {
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
            maxWidth: metrics.wide ? 460 : 280, height: metrics.wide ? 120 : 96)
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
                Text(metaLine(details, seasons: model.header?.seasons ?? []))
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
                if let description = details.description {
                    Synopsis(text: description, lines: 3)
                }
                PlayBlock(
                    target: model.header?.playTarget, versions: versions?.snapshot, failure: versions?.failure,
                    wide: metrics.wide,
                    play: {
                        if let route = versions?.play() {
                            playing = route
                        } else {
                            // Sources still answering: play the pick once it settles.
                            Task { playing = await versions?.settledPick() }
                        }
                    },
                    addSource: { showAddons = true })
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
    /// movie, Watched. They wrap onto a second line on narrow screens.
    private func actions(_ details: TitleDetails) -> some View {
        let header = model.header
        let inLibrary = header?.inLibrary ?? false
        let watched = header?.watched ?? false
        let count = versions?.snapshot.map { $0.rows.count + ($0.recommended == nil ? 0 : 1) } ?? 0
        return FlowRow(spacing: 12) {
            Button { showVersions = true } label: {
                Label(count > 0 ? "Versions (\(count))" : "Versions", glyph: "layers")
            }
            .disabled(versions == nil)
            if let trailer = details.trailers.first, let url = URL(string: trailer.url) {
                Button { openURL(url) } label: { Label("Trailer", glyph: "film") }
            }
            Button { model.setInLibrary(!inLibrary); sync.schedule() } label: {
                Label(inLibrary ? "In Library" : "Add to Library", glyph: inLibrary ? "bookmark-check" : "bookmark-plus")
            }
            if details.kind == "movie" {
                Button { model.setWatched(details.id, !watched); sync.schedule() } label: {
                    Label(watched ? "Watched" : "Mark as watched", glyph: watched ? "circle-check" : "circle-check")
                }
            }
        }
        .buttonStyle(PillButtonStyle())
    }

    @ViewBuilder private var lower: some View {
        if let details {
            let series = !(model.header?.seasons.isEmpty ?? true)
            if metrics.wide && series {
                HStack(alignment: .top, spacing: Theme.Space.xl) {
                    Credits(details: details, series: true).frame(width: 300, alignment: .leading)
                    episodes
                }
                .padding(.horizontal, metrics.gutter)
            } else {
                // Phone: what to watch first, then who made it.
                VStack(alignment: .leading, spacing: Theme.Space.l) {
                    if series { episodes }
                    Credits(details: details, series: series)
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
                .foregroundStyle(Theme.Palette.text)
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
                Button("More") { withAnimation(Theme.Motion.panel) { open = true } }
                    .buttonStyle(.plain)
                    .font(.text(Theme.type.callout, weight: .semibold, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.text)
                    .accessibilityHint("Shows the whole synopsis")
            }
        }
    }
}

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
    let versions: VersionsSnapshot?
    let failure: String?
    let wide: Bool
    let play: () -> Void
    let addSource: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            if versions?.done == true, versions?.recommended == nil {
                Button(action: addSource) {
                    Label("Add a source", glyph: "plus")
                        .frame(minWidth: wide ? 160 : nil, maxWidth: wide ? nil : .infinity)
                }
                .buttonStyle(PillButtonStyle(prominent: true))
            } else {
                Button(action: play) {
                    HStack(spacing: 10) {
                        Glyph("play-fill")
                        Text(target?.label ?? "Play")
                        if let resume = target?.resumeMs, resume > 0 {
                            Text("from \(clock(ms: resume))").fontWeight(.regular).opacity(0.8)
                        }
                    }
                    .frame(minWidth: wide ? 160 : nil, maxWidth: wide ? nil : .infinity)
                }
                .buttonStyle(PillButtonStyle(prominent: true))
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
            .font(.text(Theme.type.callout, relativeTo: .callout))
            .lineLimit(2)
            .fixedSize(horizontal: false, vertical: true)
        }
    }
}

/// "720p  472 MB  (dot) Torrent, 153 seeds  via Torrentio", or why there is none yet.
private struct VersionLine: View {
    let versions: VersionsSnapshot?

    var body: some View {
        if let best = versions?.recommended {
            let dot = Text("\u{25CF}").font(.system(size: 9)).foregroundStyle(VersionWords.statusColor(best.status))
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
