import Foundation
import SwiftUI

/// Browse one catalog at a time (plans/design-v3-phase7-apple.md 4). The Mac and iPad: the kinds, the
/// catalog and the genre over a grid of covers, the selected one previewed at the right. The iPhone:
/// the same filters over two columns. TV keeps its chip rows over a poster grid (D9).
struct DiscoverScreen: View {
    @State private var discover: LiveRecord<DiscoverSnapshot>
    /// First run opens the recommended addons here too.
    @State private var addons: LiveRecord<AddonsSnapshot>
    @Environment(\.metrics) private var metrics
    @Environment(SyncModel.self) private var sync
    private let core: CoreClient
    @Environment(ProfileSession.self) private var profiles
    @State private var onboarding = false
    @Environment(\.linkRequests) private var links
    @Environment(\.openTitle) private var openTitle
    /// The Mac's and iPad's selected poster, previewed beside the grid.
    @State private var selection = DiscoverSelection()
    @State private var width: CGFloat = 0

    init(core: CoreClient) {
        self.core = core
        _discover = State(initialValue: LiveRecord("discover", core: core))
        _addons = State(initialValue: LiveRecord("addons", core: core))
    }

    var body: some View {
        page
            .background(Theme.Palette.bg)
            .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
            .task(id: discover.taskId) { discover.start() }
            .task(id: addons.taskId) { addons.start() }
            .onChange(of: addons.snapshot?.fresh == true, initial: true) { _, fresh in if fresh { onboarding = true } }
            // A title page's genre chip (DiscoverRequest): sent once, when there is a snapshot.
            .onChange(of: links?.discover, initial: true) { follow() }
            .onChange(of: discover.snapshot != nil) { follow() }
            // A new kind, catalog or genre selects its first poster; more of the same keeps the selection.
            .onChange(of: discover.snapshot, initial: true) { _, s in
                guard let s else { return }
                selection.update(scope: s.list, keys: s.items.keyed.map(\.key))
            }
            // The previewed item's details come from the core's catalog answer (C44a): asked again for each
            // new item or list, since a new list clears them.
            .onChange(of: previewAsk, initial: true) { _, ask in
                if let ask { discover.command(["type": "preview", "id": ask.id]) }
            }
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

    @ViewBuilder private var page: some View {
        #if os(tvOS)
        scroll { tvFilters($0) } grid: { s in
            PosterGrid(tiles: s.items, onEnd: { loadMore(s) })
        }
        #else
        if metrics.wide {
            HStack(alignment: .top, spacing: Theme.Preview.gap) {
                scroll { DiscoverFilters(snapshot: $0, select: select) } grid: { s in
                    DiscoverGrid(
                        tiles: s.items, columns: DiscoverLayout.columns(width: width), selects: preview != nil,
                        selection: $selection, onEnd: { loadMore(s) })
                }
                if let preview {
                    DiscoverPreview(tile: preview, details: discover.snapshot?.preview) { openTitle(TitleRoute(preview)) }
                        .padding(.top, Theme.Space.l)
                        .padding(.trailing, DiscoverLayout.margin)
                }
            }
        } else {
            scroll { DiscoverFilters(snapshot: $0, select: select) } grid: { s in
                PhoneDiscoverGrid(tiles: s.items, onEnd: { loadMore(s) })
            }
        }
        #endif
    }

    /// The selected poster, where the window holds the preview beside four columns (the Mac and iPad).
    private var preview: Tile? {
        #if os(tvOS)
        return nil
        #else
        guard metrics.wide, DiscoverLayout.previewFits(width: width), let key = selection.key else { return nil }
        return discover.snapshot?.items.keyed.first { $0.key == key }?.tile
        #endif
    }

    /// The item to ask the core's details of, with its list, so a new list asks again.
    private var previewAsk: DiscoverPreviewAsk? {
        guard let s = discover.snapshot, let tile = preview else { return nil }
        return DiscoverPreviewAsk(id: tile.id, list: s.list)
    }

    /// The title, the filters and the grid with its states, scrolling.
    private func scroll<Filters: View, Grid: View>(
        @ViewBuilder filters: @escaping (DiscoverSnapshot) -> Filters,
        @ViewBuilder grid: @escaping (DiscoverSnapshot) -> Grid
    ) -> some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: metrics.wide ? 24 : Theme.Space.m) {
                ScreenTitle(text: "Discover", desktop: true)
                if let s = discover.snapshot {
                    // Without catalogs there is nothing to filter: only the way on shows.
                    if !s.kinds.isEmpty { filters(s) }
                    content(s, grid: grid)
                } else if discover.failure != nil {
                    // The core's words can name a host or an addon: the shared line instead.
                    StateLine(symbol: "triangle-alert", text: DiscoverWords.failed, action: ("Try again", discover.retry))
                }
            }
            .padding(.leading, metrics.wide ? DiscoverLayout.margin : metrics.gutter)
            // Beside the preview the grid ends at the gap; the preview keeps the margin.
            .padding(.trailing, metrics.wide ? (preview == nil ? DiscoverLayout.margin : 0) : metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .topScrim()
    }

    @ViewBuilder private func content<Grid: View>(_ s: DiscoverSnapshot, grid: (DiscoverSnapshot) -> Grid) -> some View {
        if s.kinds.isEmpty {
            CatalogSetup(
                title: "Nothing to browse yet",
                text: "Discover lists the catalogs of the addons you have on, and none of them has one. Set up Marquee offers the recommended addons.",
                titleSize: Theme.type.title2, core: core, setUp: { onboarding = true })
        } else {
            if s.items.isEmpty, s.state == .loading {
                skeleton
            } else {
                grid(s)
            }
            if s.state == .loading, !s.items.isEmpty {
                ProgressView().controlSize(.large).frame(maxWidth: .infinity).padding(Theme.Space.l)
            }
            if let words = DiscoverWords.state(s) {
                switch s.state {
                case .failed:
                    StateLine(symbol: "triangle-alert", text: words, action: ("Try again", { discover.command(["type": "loadMore"]) }))
                case .end where s.items.isEmpty:
                    StateLine(symbol: "inbox", text: words)
                default:
                    StateLine(symbol: "list-filter", text: words)
                }
            }
        }
    }

    /// A first page loading: one row of blank covers.
    @ViewBuilder private var skeleton: some View {
        #if os(tvOS)
        ProgressView().controlSize(.large).frame(maxWidth: .infinity).padding(Theme.Space.l)
        #else
        if metrics.wide {
            DiscoverSkeleton(count: DiscoverLayout.columns(width: width), size: DiscoverLayout.poster, gap: DiscoverLayout.gap, radius: Theme.TileDesktop.radius)
        } else {
            DiscoverSkeleton(
                count: Theme.Tile.Grid.columns, size: DiscoverLayout.phonePoster(width: width - 2 * metrics.gutter),
                gap: Theme.Tile.Grid.gap, radius: Theme.Tile.radius)
        }
        #endif
    }

    private func loadMore(_ s: DiscoverSnapshot) {
        if s.state == .ready { discover.command(["type": "loadMore"]) }
    }

    #if os(tvOS)
    @ViewBuilder private func tvFilters(_ s: DiscoverSnapshot) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            ChipRow(
                label: "Type", chips: DiscoverWords.kinds(s).map { Chip(id: $0.id, title: $0.title) },
                isSelected: { $0 == s.kind }, select: { select(["kind": $0]) })
            if !s.catalogs.isEmpty {
                ChipRow(
                    label: "Catalog", chips: DiscoverWords.catalogs(s).map { Chip(id: $0.id, title: $0.title) },
                    isSelected: { $0 == s.catalog }, select: { select(["catalog": $0]) })
            }
            if !s.genres.isEmpty {
                // "" stands for All, which a catalog that needs a genre does not offer.
                ChipRow(
                    label: "Genre", chips: DiscoverWords.genres(s).map { Chip(id: $0.id ?? "", title: $0.title) },
                    isSelected: { $0 == (s.genre ?? "") },
                    select: { select(["genre": $0.isEmpty ? NSNull() : $0]) })
            }
            if let line = DiscoverWords.missingGenre(s.missingGenre) {
                Text(line)
                    .textFont(13, relativeTo: .footnote)
                    .foregroundStyle(Theme.Palette.dim)
            }
        }
    }
    #endif

    private func follow() {
        guard let links else { return }
        DiscoverRequest.follow(&links.discover, snapshot: discover.snapshot) { discover.command($0) }
    }

    private func select(_ fields: [String: any Sendable]) {
        var command = fields
        command["type"] = "select"
        discover.command(command)
    }
}
