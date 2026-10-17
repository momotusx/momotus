import SwiftUI

/// Title page: art with the decision on top (logo, meta, synopsis, Play and
/// the version under it, library), then credits and the episode contents.
struct TitleScreen: View {
    @State private var model: TitleModel
    let core: CoreClient
    @Environment(\.metrics) private var metrics
    @Environment(\.openURL) private var openURL
    @State private var width: CGFloat = 0
    /// Versions for the play target: the line under Play, Play itself, and the Versions page.
    @State private var versions: VersionsModel?
    @State private var playing: PlayRoute?
    @State private var showVersions = false

    init(route: TitleRoute, core: CoreClient) {
        _model = State(initialValue: TitleModel(route: route, core: core))
        self.core = core
    }

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.l) {
                hero
                if let failure = model.failure {
                    StateLine(symbol: "exclamationmark.triangle", text: failure)
                        .padding(.horizontal, metrics.gutter)
                }
                lower
            }
            .padding(.bottom, Theme.Space.xl)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
        .task(id: model.taskId) { await model.run() }
        .task(id: playTarget?.videoId) { await openVersions() }
        #if DEBUG
        .onChange(of: versions?.snapshot?.recommended?.key) { _, key in
            if DebugHooks.autoPlay, key != nil, playing == nil { playing = versions?.play() }
            if DebugHooks.showVersions, key != nil { showVersions = true }
        }
        #endif
        .fullBleedTop()
        .navigationDestination(item: $playing) { route in
            PlayerScreen(route: route, versions: versions, core: core)
        }
        .navigationDestination(isPresented: $showVersions) {
            if let versions {
                VersionsScreen(model: versions, heading: versionsHeading, core: core)
            }
        }
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

    /// One Versions view per play target; it stays open while its playback runs.
    private func openVersions() async {
        guard let target = playTarget else { versions = nil; return }
        let model = VersionsModel(target: target, core: core)
        versions = model
        await model.run()
    }

    /// "S2:E3 What Is Dead May Never Die" or the movie's name.
    private var versionsHeading: String {
        guard let target = versions?.target, let season = target.season, let number = target.episode else {
            return details?.name ?? model.route.name
        }
        let title = model.episodes.first { $0.videoId == target.videoId }?.title
        return ["S\(season):E\(number)", title].compactMap { $0 }.joined(separator: " ")
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
                Backdrop(url: details?.background, height: heroHeight, wide: true)
                decision.frame(maxWidth: 580, alignment: .leading)
                    .padding(.horizontal, metrics.gutter)
                    .padding(.bottom, Theme.Space.m)
            }
        } else {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                ZStack(alignment: .bottomLeading) {
                    Backdrop(url: details?.background, height: heroHeight, wide: false)
                    logo.padding(.horizontal, metrics.gutter)
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
                    Text(description)
                        .font(.text(Theme.type.body))
                        .foregroundStyle(Theme.Palette.text)
                        .lineLimit(metrics.wide ? 3 : 6)
                        .fixedSize(horizontal: false, vertical: true)
                }
                PlayBlock(
                    target: model.header?.playTarget, versions: versions?.snapshot, failure: versions?.failure,
                    wide: metrics.wide
                ) {
                    playing = versions?.play()
                }
                actions(details)
            } else if case .failed(let reason) = model.header?.state {
                StateLine(symbol: "exclamationmark.triangle", text: reason, action: ("Try again", model.retry))
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
                Label(count > 0 ? "Versions (\(count))" : "Versions", systemImage: "square.stack.3d.up")
            }
            .disabled(versions == nil)
            if let trailer = details.trailers.first, let url = URL(string: trailer.url) {
                Button { openURL(url) } label: { Label("Trailer", systemImage: "film") }
            }
            Button { model.setInLibrary(!inLibrary) } label: {
                Label(inLibrary ? "In Library" : "Add to Library", systemImage: inLibrary ? "bookmark.fill" : "bookmark")
            }
            if details.kind == "movie" {
                Button { model.setWatched(details.id, !watched) } label: {
                    Label(watched ? "Watched" : "Mark watched", systemImage: watched ? "checkmark.circle.fill" : "checkmark.circle")
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
                VStack(alignment: .leading, spacing: Theme.Space.l) {
                    Credits(details: details, series: series)
                    if series { episodes }
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
            setWatched: model.setWatched)
    }
}

/// Backdrop art faded into the page.
private struct Backdrop: View {
    let url: String?
    let height: CGFloat
    let wide: Bool

    var body: some View {
        let bg = Theme.Palette.bg
        ArtImage(url: url, maxPixels: ArtSize.backdrop)
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
                    stops: [.init(color: bg.opacity(wide ? 0 : 0.45), location: 0), .init(color: .clear, location: 0.3), .init(color: bg.opacity(0.85), location: 0.82), .init(color: bg, location: 1)],
                    startPoint: .top, endPoint: .bottom)
            }
            .accessibilityHidden(true)
    }
}

/// Play or Resume with the resume point, and the chosen version under it.
private struct PlayBlock: View {
    let target: PlayTarget?
    let versions: VersionsSnapshot?
    let failure: String?
    let wide: Bool
    let play: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Button(action: play) {
                HStack(spacing: 10) {
                    Image(systemName: "play.fill")
                    Text(target?.label ?? "Play")
                    if let resume = target?.resumeMs, resume > 0 {
                        Text("from \(clock(ms: resume))").fontWeight(.regular).opacity(0.8)
                    }
                }
                .frame(maxWidth: wide ? nil : .infinity)
            }
            .buttonStyle(PillButtonStyle(prominent: true))
            .disabled(versions?.recommended == nil)
            if let failure {
                Label("Versions could not load: \(failure)", systemImage: "exclamationmark.triangle")
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
            } else {
                VersionLine(versions: versions)
                    .font(.text(Theme.type.callout, relativeTo: .callout))
            }
        }
    }
}

/// "720p  472 MB  (dot) Torrent, 153 seeds  via Torrentio", or why there is none yet.
private struct VersionLine: View {
    let versions: VersionsSnapshot?

    var body: some View {
        if let best = versions?.recommended {
            let dot = Text(Image(systemName: "circle.fill")).font(.system(size: 8)).foregroundStyle(VersionWords.statusColor(best.status))
            let via = Text("via \(best.via)").foregroundStyle(Theme.Palette.muted)
            Text("\(VersionWords.line(best))   \(dot) \(VersionWords.status(best.status))   \(via)")
                .foregroundStyle(Theme.Palette.text)
                .accessibilityLabel("\(VersionWords.line(best)), \(VersionWords.status(best.status)), via \(best.via)")
        } else if let versions, versions.done {
            Label(
                versions.hidden.count > 0
                    ? "No version plays here: \(VersionWords.hidden(versions.hidden))"
                    : "No version found: your addons have no source for this",
                systemImage: "exclamationmark.circle")
                .foregroundStyle(Theme.Palette.muted)
        } else {
            Label(
                versions?.pendingAddons.isEmpty == false
                    ? "Asking \(versions!.pendingAddons.joined(separator: ", "))"
                    : "Finding the best version",
                systemImage: "clock")
                .foregroundStyle(Theme.Palette.muted)
        }
    }
}
