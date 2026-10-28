// iOS and macOS only: TV forms show no downloads (DownloadGate).
#if !os(tvOS)
import SwiftUI

/// Where Library leads (plans/offline-downloads.md 3.4): the Downloads page. Home's offline banner asks
/// for it through `LinkRequests.library`.
enum LibraryRoute: Hashable {
    case downloads
}

/// The way to Downloads at the top of Library, as an iOS grouped row.
struct DownloadsRow: View {
    let keeper: DownloadKeeper
    let open: () -> Void

    var body: some View {
        Button(action: open) {
            HStack(spacing: 13) {
                Glyph("arrow-down-to-line", size: 20)
                Text("Downloads").textFont(Theme.type.body)
                Spacer(minLength: 0)
                if let left = keeper.snapshot?.moving, left > 0 {
                    Text("\(left) downloading")
                        .textFont(Theme.type.callout, relativeTo: .callout)
                        .foregroundStyle(Theme.Palette.dim)
                        .monospacedDigit()
                }
                Glyph("chevron-right", size: 14).foregroundStyle(Theme.Palette.dim)
            }
            .foregroundStyle(Theme.Palette.text)
            .padding(.horizontal, 16)
            .frame(minHeight: 51)
            .background(Theme.Palette.surface, in: RoundedRectangle(cornerRadius: 26, style: .continuous))
        }
        .buttonStyle(FocusPlainStyle(shape: RoundedRectangle(cornerRadius: 26, style: .continuous)))
        .accessibilityHint("Shows what is downloaded on this device")
    }
}

/// What a row of the Downloads screens can do: play it, choose another version, open a series.
@MainActor private struct RowActions {
    let keeper: DownloadKeeper
    let kids: Bool
    let play: (DownloadRow) -> Void
    let choose: (DownloadRow) -> Void

    func entries(_ row: DownloadRow) -> [DownloadMenuEntry] {
        DownloadCopy.control(DownloadCopy.badge(row)).items.map { item in
            DownloadMenuEntry(id: item.rawValue, label: item.label, glyph: item.glyph) {
                switch item {
                case .play: play(row)
                case .another, .choose: choose(row)
                default: _ = keeper.run(item, id: row.id)
                }
            }
        }
    }

    /// A done or running download plays (a running one from what is there so far).
    static func plays(_ row: DownloadRow) -> Bool { row.state == .done || row.state == .running }
}

/// The Downloads page: the storage line over its bar, what is downloading, then the titles kept, newest
/// first. A kids profile sees the same page with Play only.
struct DownloadsScreen: View {
    let keeper: DownloadKeeper
    let core: CoreClient
    @Environment(\.metrics) private var metrics
    @Environment(ProfileSession.self) private var profiles: ProfileSession?
    @State private var player = DownloadPlayer()
    @State private var series: String?
    /// A series whose downloads go, once its confirmation says yes.
    @State private var deleting: DownloadTitle?

    var body: some View {
        DownloadsList(title: "Downloads") {
            if let snapshot = keeper.snapshot {
                Storage(storage: snapshot.storage).listRow()
                if !snapshot.downloading.isEmpty {
                    downloading(snapshot)
                }
                if !snapshot.titles.isEmpty {
                    ForEach(snapshot.titles) { title in titleRow(title) }
                } else if snapshot.downloading.isEmpty {
                    Empty().listRow()
                }
            } else {
                NotOpen(keeper: keeper).listRow()
            }
        }
        .modifier(player.destinations(core: core, keeper: keeper))
        .navigationDestination(item: $series) { id in
            DownloadsSeriesPage(itemId: id, keeper: keeper, core: core)
        }
        .downloadConfirm(item: $deleting, title: { DownloadCopy.deleteSeries($0.rows.count, $0.name) }) {
            keeper.act(.remove, .title($0.itemId))
        }
        .downloadProblem(keeper)
        .downloadsPage(keeper)
    }

    private var actions: RowActions {
        RowActions(keeper: keeper, kids: kids, play: { player.play($0, core: core) }, choose: { player.choose($0, core: core) })
    }

    private var kids: Bool { profiles?.snapshot?.current?.kids == true }

    @ViewBuilder private func downloading(_ snapshot: DownloadsSnapshot) -> some View {
        HStack(alignment: .firstTextBaseline) {
            SectionHeading(text: "Downloading")
            Spacer()
            if !kids {
                let anyGoing = snapshot.downloading.contains { $0.state != .paused }
                Button(anyGoing ? "Pause all" : "Resume all") { keeper.act(anyGoing ? .pause : .resume, .all) }
                    .buttonStyle(.textLink)
                    .textFont(Theme.type.callout, weight: .semibold, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.key)
            }
        }
        .padding(.top, Theme.Space.s)
        .listRow()
        ForEach(snapshot.downloading) { row in
            DownloadRowView(row: row, title: row.name, subtitle: row.episode == nil ? nil : DownloadCopy.episodeName(row),
                            line: DownloadCopy.progressLine(row), art: row.still ?? row.poster, wideArt: row.still != nil,
                            progress: DownloadCopy.badge(row).progress, actions: actions)
        }
        if !snapshot.titles.isEmpty {
            SectionHeading(text: "Downloaded").padding(.top, Theme.Space.s).listRow()
        }
    }

    @ViewBuilder private func titleRow(_ title: DownloadTitle) -> some View {
        if title.kind == "movie", let row = title.rows.first {
            DownloadRowView(row: row, title: title.name, subtitle: nil, line: DownloadCopy.titleLine(title),
                            art: title.poster ?? row.poster, wideArt: false, progress: nil, actions: actions)
        } else {
            Button { series = title.itemId } label: {
                HStack(spacing: 14) {
                    Poster(url: title.poster, wide: false)
                    VStack(alignment: .leading, spacing: 3) {
                        Text(title.name).textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline).lineLimit(2)
                        Text(DownloadCopy.titleLine(title))
                            .textFont(Theme.type.callout, relativeTo: .callout)
                            .foregroundStyle(Theme.Palette.muted)
                            .monospacedDigit()
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    Glyph("chevron-right", size: 14).foregroundStyle(Theme.Palette.dim)
                }
                .foregroundStyle(Theme.Palette.text)
                .contentShape(Rectangle())
            }
            .buttonStyle(.row)
            .accessibilityHint("Shows its episodes")
            .listRow()
            #if os(macOS)
            .contextMenu {
                if !kids { Button("Delete all", glyph: DownloadItem.delete.glyph) { deleting = title } }
            }
            #endif
            // A whole series asks first (plans/offline-downloads.md 3.4).
            .deleteSwipe(enabled: !kids) { deleting = title }
        }
    }
}

/// A series' downloaded episodes, grouped by season; "Delete all" in the page's menu asks first.
struct DownloadsSeriesPage: View {
    let itemId: String
    let keeper: DownloadKeeper
    let core: CoreClient
    @Environment(ProfileSession.self) private var profiles: ProfileSession?
    @State private var player = DownloadPlayer()
    @State private var asking = false

    var body: some View {
        DownloadsList(title: title?.name ?? "Downloads") {
            if keeper.snapshot == nil {
                NotOpen(keeper: keeper).listRow()
            } else if let title {
                ForEach(seasons(title), id: \.0) { season, rows in
                    SectionHeading(text: season.map(seasonName) ?? "Episodes").padding(.top, Theme.Space.s).listRow()
                    ForEach(rows) { row in
                        DownloadRowView(row: row, title: DownloadCopy.episodeName(row), subtitle: nil, line: DownloadCopy.episodeLine(row),
                                        art: row.still ?? row.poster, wideArt: true, progress: nil, actions: actions)
                    }
                }
            } else {
                // Its last download went: nothing left to show.
                Empty().listRow()
            }
        }
        .toolbar {
            if !kids, let title {
                ToolbarItem(placement: .primaryAction) {
                    Menu {
                        Button("Delete all", glyph: DownloadItem.delete.glyph) { asking = true }
                    } label: {
                        Label("More", glyph: "ellipsis")
                    }
                    .accessibilityLabel("More")
                    .downloadConfirm(DownloadCopy.deleteSeries(title.rows.count, title.name), isPresented: $asking) {
                        keeper.act(.remove, .title(itemId))
                    }
                }
            }
        }
        .modifier(player.destinations(core: core, keeper: keeper))
        .downloadProblem(keeper)
        .downloadsPage(keeper)
    }

    private var title: DownloadTitle? { keeper.snapshot?.titles.first { $0.itemId == itemId } }
    private var kids: Bool { profiles?.snapshot?.current?.kids == true }

    private var actions: RowActions {
        RowActions(keeper: keeper, kids: kids, play: { player.play($0, core: core) }, choose: { player.choose($0, core: core) })
    }

    /// Seasons in order, specials last; each season's episodes in order.
    private func seasons(_ title: DownloadTitle) -> [(Int?, [DownloadRow])] {
        let groups = Dictionary(grouping: title.rows, by: \.season)
        let keys = groups.keys.sorted { a, b in
            switch (a, b) {
            case (nil, _): false
            case (_, nil): true
            case let (a?, b?): (a == 0 ? Int.max : a) < (b == 0 ? Int.max : b)
            }
        }
        return keys.map { key in (key, (groups[key] ?? []).sorted { ($0.episode ?? 0) < ($1.episode ?? 0) }) }
    }
}

/// One download: its art, its words, a progress line while it downloads, and its control. A done or
/// running row plays when tapped; the control's menu and, on iOS, a swipe delete it.
private struct DownloadRowView: View {
    let row: DownloadRow
    let title: String
    let subtitle: String?
    let line: String
    let art: String?
    let wideArt: Bool
    let progress: Double?
    let actions: RowActions

    var body: some View {
        HStack(spacing: 10) {
            Button { actions.play(row) } label: { face }
                .buttonStyle(.row)
                .disabled(!RowActions.plays(row))
                .accessibilityElement(children: .combine)
                .accessibilityHint(RowActions.plays(row) ? "Plays it" : "")
            if !actions.kids {
                DownloadControlButton(control: DownloadCopy.control(DownloadCopy.badge(row)), look: .bare, entries: actions.entries(row),
                                      name: [title, subtitle].compactMap { $0 }.joined(separator: " "))
            }
        }
        .listRow()
        #if os(macOS)
        .contextMenu {
            if !actions.kids { DownloadMenuItems(entries: actions.entries(row)) }
        }
        #endif
        .deleteSwipe(enabled: !actions.kids) { actions.keeper.act(.remove, .ids([row.id])) }
    }

    private var face: some View {
        HStack(spacing: 14) {
            Poster(url: art, wide: wideArt)
            VStack(alignment: .leading, spacing: 3) {
                Text(title).textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline).lineLimit(2)
                if let subtitle {
                    Text(subtitle).textFont(Theme.type.callout, relativeTo: .callout).lineLimit(1)
                }
                Text(line)
                    .textFont(Theme.type.callout, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.muted)
                    .monospacedDigit()
                    .lineLimit(2)
                // The subtitles kept with it, quietly (plans/offline-downloads.md 4.10).
                if let subtitles = DownloadCopy.subtitlesLine(row.subtitles) {
                    Text(subtitles)
                        .textFont(Theme.type.caption, relativeTo: .caption)
                        .foregroundStyle(Theme.Palette.dim)
                        .lineLimit(2)
                }
                if let progress {
                    ProgressLine(value: progress).frame(maxWidth: 220).padding(.top, 4).accessibilityHidden(true)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .foregroundStyle(Theme.Palette.text)
        .contentShape(Rectangle())
    }
}

/// A poster at the Library grid's small size, or an episode's 16:9 still.
private struct Poster: View {
    let url: String?
    let wide: Bool

    var body: some View {
        ArtImage(url: url, maxPixels: 320)
            .frame(width: wide ? 112 : 56, height: wide ? 63 : 84)
            .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
            .accessibilityHidden(true)
    }
}

private struct SectionHeading: View {
    let text: String

    var body: some View {
        Text(text)
            .display(Theme.type.title3)
            .foregroundStyle(Theme.Palette.text)
            .textCase(nil)
            .accessibilityAddTraits(.isHeader)
    }
}

/// "Downloads use 12.4 GB · 48 GB free on this device" over a thin bar: downloads white, the rest of the
/// used space 30 percent white, free space the track.
private struct Storage: View {
    let storage: DownloadStorage
    @State private var total: Int64?

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(DownloadCopy.storage(storage))
                .textFont(Theme.type.callout, relativeTo: .callout)
                .foregroundStyle(Theme.Palette.muted)
                .monospacedDigit()
                .fixedSize(horizontal: false, vertical: true)
            if let total, total > 0 {
                let mine = Double(storage.downloadsBytes) / Double(total)
                let used = Double(max(0, total - (storage.freeBytes ?? 0))) / Double(total)
                GeometryReader { geo in
                    ZStack(alignment: .leading) {
                        Capsule().fill(.white.opacity(0.1))
                        Capsule().fill(.white.opacity(0.3)).frame(width: geo.size.width * min(1, max(used, mine)))
                        Capsule().fill(.white).frame(width: geo.size.width * min(1, mine))
                    }
                }
                .frame(height: 4)
                .accessibilityHidden(true)
            }
        }
        .task {
            let values = try? CoreClient.dataDirectory().resourceValues(forKeys: [.volumeTotalCapacityKey])
            total = values?.volumeTotalCapacity.map(Int64.init)
        }
    }
}

extension View {
    /// A Downloads page on screen: the shell's offline banner offers no way to where the user already is.
    func downloadsPage(_ keeper: DownloadKeeper) -> some View {
        onAppear { keeper.downloadsPages += 1 }
            .onDisappear { keeper.downloadsPages -= 1 }
    }

    /// A download op the core refused, over the page's bottom edge until the next op or Dismiss (review r3 N1).
    func downloadProblem(_ keeper: DownloadKeeper?) -> some View {
        safeAreaInset(edge: .bottom) {
            if let keeper, let failure = keeper.failure {
                StateLine(symbol: "triangle-alert", text: failure, action: ("Dismiss", keeper.dismissFailure))
                    .flatCard()
                    .padding(.horizontal, Theme.Space.m)
                    .padding(.bottom, Theme.Space.s)
                    .accessibilityElement(children: .contain)
            }
        }
    }
}

/// The view has no snapshot yet (a core opened again): a spinner a moment on, so a quick open does not
/// flash one, or why it did not open with Try again; never "Nothing downloaded" (review r3 N5).
private struct NotOpen: View {
    let keeper: DownloadKeeper
    @State private var waited = false

    var body: some View {
        if let failure = keeper.view?.failure {
            StateLine(symbol: "triangle-alert", text: failure, action: ("Try again", { keeper.view?.retry() }))
        } else {
            ProgressView()
                .controlSize(.large)
                .tint(Theme.Palette.text)
                .opacity(waited ? 1 : 0)
                .frame(maxWidth: .infinity)
                .padding(Theme.Space.l)
                .accessibilityLabel(DownloadCopy.loading)
                .task {
                    try? await Task.sleep(for: .milliseconds(250))
                    waited = true
                }
        }
    }
}

/// Nothing downloaded: what the page is for, centered.
private struct Empty: View {
    @Environment(\.metrics) private var metrics

    var body: some View {
        VStack(spacing: Theme.Space.s) {
            Glyph("arrow-down-to-line", size: metrics.wide ? 48 : 40)
                .foregroundStyle(Theme.Palette.muted)
            Text(DownloadCopy.emptyTitle)
                .display(Theme.type.title2)
                .foregroundStyle(Theme.Palette.text)
            Text(DownloadCopy.emptyBody)
                .textFont(Theme.type.body)
                .foregroundStyle(Theme.Palette.muted)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 420)
        }
        .frame(maxWidth: .infinity)
        .padding(.top, metrics.wide ? Theme.Space.xxl : Theme.Space.xl)
    }
}

/// The page's list: rows on the page with no list chrome (so iOS can swipe them), the title on top,
/// held to the reading width with the page's gutters.
private struct DownloadsList<Content: View>: View {
    let title: String
    @ViewBuilder let content: Content
    @Environment(\.metrics) private var metrics

    var body: some View {
        List {
            ScreenTitle(text: title).listRow()
            content
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .environment(\.defaultMinListRowHeight, 0)
        .frame(maxWidth: 880)
        .frame(maxWidth: .infinity)
        .background(Theme.Palette.bg)
        .topScrim()
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
    }
}

extension View {
    /// A row on the page: no background, separator or list insets beyond the page's gutters.
    fileprivate func listRow() -> some View {
        modifier(ListRowChrome())
    }

    /// iOS: swipe to delete, with no destructive role: the neutral 16 percent fill and the white word
    /// (design/DECISION.md, Destructive).
    @ViewBuilder fileprivate func deleteSwipe(enabled: Bool, _ delete: @escaping () -> Void) -> some View {
        #if os(iOS)
        if enabled {
            swipeActions(edge: .trailing, allowsFullSwipe: true) {
                Button(action: delete) { Text("Delete") }
                    .tint(Color(white: 0.16))
            }
        } else {
            self
        }
        #else
        self
        #endif
    }
}

private struct ListRowChrome: ViewModifier {
    @Environment(\.metrics) private var metrics

    func body(content: Content) -> some View {
        content
            .listRowBackground(Color.clear)
            .listRowSeparator(.hidden)
            .listRowInsets(EdgeInsets(top: 6, leading: metrics.gutter, bottom: 6, trailing: metrics.gutter))
    }
}

/// Plays a download through the normal play path (the Versions view recommends the file on this
/// device at once), and opens Versions to choose another version to keep.
@MainActor @Observable private final class DownloadPlayer {
    var playing: PlayRoute?
    var versions: VersionsModel?
    var keeping: VersionsModel?
    var showVersions = false
    var showKeeping = false

    func play(_ row: DownloadRow, core: CoreClient) {
        let model = VersionsModel(target: target(row), core: core)
        model.start()
        versions = model
        switch TitlePlay.of(model.snapshot) {
        case .player(let key): playing = key.flatMap { model.play($0) } ?? model.pending()
        case .versions: showVersions = true
        case .inert: break
        }
    }

    func choose(_ row: DownloadRow, core: CoreClient) {
        let model = VersionsModel(target: target(row), core: core, purpose: "download")
        model.start()
        keeping = model
        showKeeping = true
    }

    private func target(_ row: DownloadRow) -> PlayTargetInfo {
        var target = PlayTargetInfo(
            kind: row.kind, itemId: row.itemId, videoId: row.videoId, title: row.name, poster: row.poster, resumeMs: nil,
            season: row.season, episode: row.episode)
        target.runtime = row.runtime
        target.released = row.released
        return target
    }

    func destinations(core: CoreClient, keeper: DownloadKeeper) -> Destinations {
        Destinations(player: self, core: core, keeper: keeper)
    }

    struct Destinations: ViewModifier {
        @Bindable var player: DownloadPlayer
        let core: CoreClient
        let keeper: DownloadKeeper

        func body(content: Content) -> some View {
            content
                .navigationDestination(item: $player.playing) { route in
                    PlayerScreen(route: route, versions: player.versions, core: core)
                }
                .navigationDestination(isPresented: $player.showVersions) {
                    if let versions = player.versions {
                        VersionsScreen(model: versions, heading: versions.target.title, core: core)
                    }
                }
                .navigationDestination(isPresented: $player.showKeeping) {
                    if let model = player.keeping {
                        VersionsScreen(model: model, heading: model.target.title, core: core, keep: { key in
                            guard let sub = model.sub else { return }
                            keeper.add(kind: model.target.kind, itemId: model.target.itemId, scope: "video", videoId: model.target.videoId, pick: (sub, key))
                            player.showKeeping = false
                        })
                    }
                }
        }
    }
}

#endif
