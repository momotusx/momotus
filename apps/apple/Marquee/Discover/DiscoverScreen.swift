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
    /// What the grid shows (plans/discover-v2.md 6.1): the core's items, except that a late list that reorders the
    /// grid waits on TV while focus is in it, and a list loading after a change keeps the one before, dimmed.
    @State private var shown: [Tile] = []
    /// The last snapshot with titles: the dimmed grid draws its tiles with its own sort's corner.
    @State private var previous: DiscoverSnapshot?
    @State private var held = false
    #if os(tvOS)
    @FocusState private var tvTile: String?
    /// Where Clear hands focus before it goes away (6.5).
    @FocusState private var tvFilter: DiscoverWords.ClearTarget?
    #endif

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
            // A title page's genre chip (DiscoverRequest): sent once, when Discover has its kinds and its subscription.
            .onChange(of: links?.discover, initial: true) { follow() }
            .onChange(of: discover.snapshot?.kinds.isEmpty == false && discover.sub != nil) { follow() }
            .onChange(of: discover.snapshot?.items, initial: true) { _, items in take(items ?? []) }
            #if os(tvOS)
            .onChange(of: tvTile) { _, key in if key == nil, held { release() } }
            #endif
            // A new kind, catalog or genre selects its first poster; more of the same keeps the selection.
            .onChange(of: discover.snapshot, initial: true) { _, s in
                // Not while a change loads: the dimmed grid stays, and so does its preview.
                guard let s, !(s.items.isEmpty && s.state == .loading) else { return }
                selection.update(scope: s.list, keys: s.items.keyed.map(\.key))
            }
            // The previewed item's details come from the core's catalog answer (C44a): asked again for each
            // new item or list, since a new list clears them.
            .onChange(of: previewAsk, initial: true) { _, ask in
                if let ask { discover.command(["type": "preview", "id": ask.id]) }
            }
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

    @ViewBuilder private var page: some View {
        #if os(tvOS)
        scroll { tvFilters($0) } grid: { s, tiles in
            PosterGrid(
                tiles: tiles,
                // The corner shows the year for the date sorts: the caption does not repeat it.
                caption: { DiscoverWords.corner(s.sort, $0) == $0.releaseInfo ? nil : $0.releaseInfo },
                onEnd: { loadMore(s) }, corner: { DiscoverWords.corner(s.sort, $0) }, focused: $tvTile)
        }
        #else
        if metrics.wide {
            HStack(alignment: .top, spacing: Theme.Preview.gap) {
                scroll { DiscoverFilters(snapshot: $0, select: select) } grid: { s, tiles in
                    // Not before the first width: the fixed columns would draw at a guess (a narrow iPad window).
                    if width > 0 {
                        DiscoverGrid(
                            tiles: tiles, columns: DiscoverLayout.columns(width: width), selects: preview != nil,
                            selection: $selection, onEnd: { loadMore(s) }, corner: { DiscoverWords.corner(s.sort, $0) })
                    }
                }
                if let preview {
                    DiscoverPreview(tile: preview, details: discover.snapshot?.preview, open: { openTitle(TitleRoute(preview)) }) { value in
                        discover.command(["type": "setInLibrary", "id": preview.id, "value": value])
                        sync.schedule()
                    }
                    .padding(.top, Theme.Space.l)
                    .padding(.trailing, DiscoverLayout.margin)
                }
            }
        } else {
            scroll { DiscoverFilters(snapshot: $0, select: select) } grid: { s, tiles in
                PhoneDiscoverGrid(tiles: tiles, onEnd: { loadMore(s) }, corner: { DiscoverWords.corner(s.sort, $0) })
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
        return DiscoverSelection.tiles(discover.snapshot, previous: previous).keyed.first { $0.key == key }?.tile
        #endif
    }

    /// The item to ask the core's details of, with its list, so a new list asks again.
    private var previewAsk: DiscoverPreviewAsk? {
        // Not while a change loads: the preview shows the list before, which the new list's details do not hold.
        guard let s = discover.snapshot, !(s.items.isEmpty && s.state == .loading), let tile = preview else { return nil }
        return DiscoverPreviewAsk(id: tile.id, list: s.list)
    }

    /// The title, the filters and the grid with its states, scrolling.
    private func scroll<Filters: View, Grid: View>(
        @ViewBuilder filters: @escaping (DiscoverSnapshot) -> Filters,
        @ViewBuilder grid: @escaping (DiscoverSnapshot, [Tile]) -> Grid
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

    @ViewBuilder private func content<Grid: View>(_ s: DiscoverSnapshot, grid: (DiscoverSnapshot, [Tile]) -> Grid) -> some View {
        if s.kinds.isEmpty {
            CatalogSetup(
                title: "Nothing to browse yet",
                text: "Discover lists the catalogs of the addons you have on, and none of them has one. Set up Momotus offers the recommended addons.",
                titleSize: Theme.type.title2, core: core, setUp: { onboarding = true })
        } else {
            if held {
                Button("More titles arrived. Show") { release() }
            }
            if s.items.isEmpty, s.state == .loading {
                if let previous {
                    // The list before, dimmed and out of focus's reach, until the new one lands.
                    grid(previous, previous.items).opacity(0.45).disabled(true).accessibilityHidden(true)
                } else {
                    skeleton
                }
            } else {
                grid(s, shown)
            }
            if s.state == .loading, !s.items.isEmpty {
                ProgressView().controlSize(.large).frame(maxWidth: .infinity).padding(Theme.Space.l)
            }
            if s.state != .loading, let scope = DiscoverWords.scope(s) {
                Text(scope)
                    .textFont(13, relativeTo: .footnote)
                    .foregroundStyle(Theme.Palette.muted)
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

    /// Shows the core's items, unless they reorder the grid while TV focus is in it: then they wait.
    private func take(_ items: [Tile]) {
        let grows = items.count >= shown.count && zip(shown, items).allSatisfy { $0.id == $1.id }
        #if os(tvOS)
        let inGrid = tvTile != nil
        #else
        let inGrid = false
        #endif
        if grows || items.isEmpty || !inGrid {
            shown = items
            held = false
        } else {
            held = true
        }
        if !items.isEmpty { previous = discover.snapshot }
    }

    private func release() {
        shown = discover.snapshot?.items ?? []
        held = false
    }

    private func loadMore(_ s: DiscoverSnapshot) {
        if s.state == .ready { discover.command(["type": "loadMore"]) }
    }

    #if os(tvOS)
    /// TV (plans/discover-v2.md 6.5): the kinds, then the services as logos, then one row of menus (Genre, Year,
    /// Clear, Sort), then the grid; each row a focus section, so the remote moves row by row.
    @ViewBuilder private func tvFilters(_ s: DiscoverSnapshot) -> some View {
        VStack(alignment: .leading, spacing: 28) {
            ChipRow(
                label: "Type", chips: DiscoverWords.kinds(s).map { Chip(id: $0.id, title: $0.title) },
                isSelected: { $0 == s.kind }, select: { select(["kind": $0]) })
            let services = DiscoverWords.services(s)
            if !services.isEmpty {
                ScrollViewReader { proxy in
                ScrollView(.horizontal) {
                    HStack(spacing: 28) {
                        ForEach(services, id: \.title) { item in
                            let on = item.id == s.platform
                            Button { select(["platform": item.id ?? NSNull()]) } label: {
                                Group {
                                    if let logo = item.logo {
                                        ArtImage(url: logo, fallback: item.title)
                                    } else {
                                        Text(item.title).textFont(23, weight: .semibold, relativeTo: .body).padding(.horizontal, 20)
                                    }
                                }
                                .frame(minWidth: 96, minHeight: 96)
                                .fixedSize()
                                .clipShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
                                .overlay {
                                    if on { RoundedRectangle(cornerRadius: 22, style: .continuous).inset(by: -5).stroke(Color.white, lineWidth: 3) }
                                }
                            }
                            .buttonStyle(.card)
                            .accessibilityLabel(item.title)
                            .accessibilityAddTraits(on ? .isSelected : [])
                            .focused($tvFilter, equals: item.id == nil ? .all : nil)
                            .id(item.title)
                        }
                    }
                    .padding(.vertical, 20)
                }
                .scrollClipDisabled()
                .focusSection()
                // A remembered service far along the rail starts in view.
                .onChange(of: s.platform, initial: true) { _, chosen in
                    if let title = services.first(where: { $0.id == chosen })?.title { proxy.scrollTo(title) }
                }
                }
            }
            HStack(spacing: 20) {
                if DiscoverWords.clearTarget(s) == .genre {
                    Menu(DiscoverWords.genreTitle(s)) {
                        ForEach(DiscoverWords.genreChoices(s), id: \.title) { item in
                            Button(item.title) { select(["genre": item.id ?? NSNull()]) }
                        }
                    }
                    .accessibilityLabel("\(s.genreLabel ?? "Genre"), \(DiscoverWords.genreTitle(s))")
                    .focused($tvFilter, equals: .genre)
                }
                if !DiscoverWords.decades(s).isEmpty {
                    Menu(DiscoverWords.decadeTitle(s)) {
                        ForEach(DiscoverWords.decades(s), id: \.title) { item in
                            Button(item.title) { select(["decade": item.id ?? NSNull()]) }
                        }
                    }
                    .accessibilityLabel("Year, \(DiscoverWords.decadeTitle(s))")
                    .focused($tvFilter, equals: .year)
                }
                if DiscoverWords.filtered(s) {
                    Button("Clear") {
                        tvFilter = DiscoverWords.clearTarget(s)
                        select(["platform": NSNull(), "genre": NSNull(), "decade": NSNull()])
                    }
                    .accessibilityLabel("Clear filters")
                }
                Spacer(minLength: 0)
                if !DiscoverWords.sorts(s).isEmpty {
                    Menu(DiscoverWords.sort(s)) {
                        ForEach(DiscoverWords.orders(s), id: \.id) { item in
                            Button(item.title) { select(["sort": item.id]) }
                        }
                        let lists = DiscoverWords.lists(s)
                        if !lists.isEmpty {
                            Section(DiscoverWords.moreLists) {
                                ForEach(lists, id: \.id) { item in
                                    Button(item.title) { select(["sort": item.id]) }
                                }
                            }
                        }
                    }
                    .accessibilityLabel("Sort, \(DiscoverWords.sort(s))")
                }
            }
            .focusSection()
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
        DiscoverRequest.follow(&links.discover, snapshot: discover.snapshot, canSend: discover.sub != nil) { discover.command($0) }
    }

    private func select(_ fields: [String: any Sendable]) {
        var command = fields
        command["type"] = "select"
        discover.command(command)
    }
}
