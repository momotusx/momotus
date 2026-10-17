import SwiftUI

/// Title page: art with the decision on top (logo, meta, synopsis, Play and
/// the version under it, library), then credits and the episode contents.
struct TitleScreen: View {
    @State private var model: TitleModel
    @Environment(\.metrics) private var metrics
    @State private var width: CGFloat = 0

    init(route: TitleRoute, core: CoreClient) {
        _model = State(initialValue: TitleModel(route: route, core: core))
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
        .fullBleedTop()
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
    }

    private var details: TitleDetails? { model.header?.details }

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
                PlayBlock(target: model.header?.playTarget, wide: metrics.wide)
                actions
            } else if case .failed(let reason) = model.header?.state {
                StateLine(symbol: "exclamationmark.triangle", text: reason, action: ("Try again", model.retry))
            } else {
                Text("Loading details")
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
            }
        }
    }

    private var actions: some View {
        let inLibrary = model.header?.inLibrary ?? false
        return HStack(spacing: 12) {
            Button {
            } label: {
                Label("Versions", systemImage: "square.stack.3d.up")
            }
            .buttonStyle(PillButtonStyle())
            .disabled(true)
            Button {
                model.setInLibrary(!inLibrary)
            } label: {
                Label(inLibrary ? "In Library" : "Add to Library", systemImage: inLibrary ? "bookmark.fill" : "bookmark")
            }
            .buttonStyle(PillButtonStyle())
        }
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
    let wide: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Button {
            } label: {
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
            .disabled(target == nil)
            Label("The best version is picked when sources answer", systemImage: "bolt")
                .font(.text(Theme.type.callout, relativeTo: .callout))
                .foregroundStyle(Theme.Palette.muted)
        }
    }
}
