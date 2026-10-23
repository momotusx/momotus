import SwiftUI

/// Search across every catalog the addons offer (plans/design-v3-phase7-apple.md 3). The core debounces
/// the query; Top results and the kind groups arrive as rows, recent searches fill the empty page. The
/// Mac and iPad: tv.apple.com's Top results cards over poster shelves. The iPhone: one list. TV keeps
/// its rows under the system's keyboard (D9).
struct SearchScreen: View {
    @State private var search: LiveRecord<SearchSnapshot>
    @FocusState private var fieldFocused: Bool
    /// Return in a field: the first result takes focus.
    @FocusState private var firstResult: Bool
    @Environment(\.metrics) private var metrics
    @Environment(\.tabShown) private var shown
    @Environment(\.headerSearchField) private var headerField
    /// The query, shared with the Mac header's field (plans/design-v3-phase7-apple.md 2).
    @Environment(SearchInput.self) private var input
    @State private var width: CGFloat = 0

    init(core: CoreClient) {
        _search = State(initialValue: LiveRecord("search", core: core))
    }

    private var query: String { input.text }
    private var typed: Bool { !query.trimmingCharacters(in: .whitespaces).isEmpty }

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: metrics.wide ? 32 : 20) {
                content
            }
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        #if os(iOS)
        .scrollDismissesKeyboard(.immediately)
        #endif
        .background(Theme.Palette.bg)
        .topScrim()
        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
        #if os(tvOS)
        // The system's inline keyboard with the results live under it, and
        // dictation, as the TV's own apps search (review apple-journey P2-8).
        .searchable(text: Bindable(input).text, prompt: Self.prompt)
        .searchSuggestions {
            ForEach(search.snapshot?.recent ?? [], id: \.self) { Label($0, glyph: "history").searchCompletion($0) }
        }
        #endif
        .task(id: search.taskId) { search.start() }
        .onChange(of: input.text) { _, value in setQuery(value) }
        // A query typed or linked before the view could take it (a director chip opening Search).
        .onChange(of: search.snapshot == nil) { _, none in
            if !none, !query.isEmpty, search.snapshot?.query != query { setQuery(query) }
        }
        // A title opened from the results covers this page: the query goes into
        // Recent, however the tile was chosen (a tap, VoiceOver, a keyboard, a remote).
        .onDisappear { if typed { search.command(["type": "remember"]) } }
        // The keyboard opens on the first visit, not over the results on the way back from a title.
        .onAppear { if query.isEmpty { fieldFocused = true } }
        #if DEBUG
        .task {
            // After the view opened and the launch's addons installed.
            guard query.isEmpty, let typed = DebugHooks.search else { return }
            try? await Task.sleep(for: .seconds(8))
            input.text = typed
        }
        #endif
        // Cmd-F, the narrow header's glyph, a window crossing the header field's width.
        .onChange(of: input.focus) { fieldFocused = true }
        .onChange(of: input.submit) { firstResult = true }
        // Out of sight the field lets go, so typing elsewhere never lands in it;
        // on the Mac it takes the cursor again each time Search shows.
        .onChange(of: shown) { _, now in
            #if os(macOS)
            fieldFocused = now
            #else
            if !now { fieldFocused = false }
            #endif
            // Leaving Search on the Mac: Recent keeps the query, then the header's field clears.
            if SearchRules.clearsOnLeave(text: query, shown: now) {
                search.command(["type": "remember"])
                input.text = ""
            }
        }
    }

    private static let prompt = "Search titles"

    private func setQuery(_ value: String) {
        search.command(["type": "setQuery", "query": value])
    }

    private var margin: CGFloat { metrics.wide ? DiscoverLayout.margin : metrics.gutter }

    @ViewBuilder private var content: some View {
        #if os(tvOS)
        tvContent
        #else
        if !metrics.wide { ScreenTitle(text: "Search").padding(.horizontal, margin) }
        // The page draws its field only where the header has none.
        if !headerField {
            SearchField(input: input, prompt: Self.prompt, focused: $fieldFocused).padding(.horizontal, margin)
        }
        if search.failure != nil {
            // The core's words can name a host or an addon: the shared line instead.
            StateLine(symbol: "triangle-alert", text: SearchWords.failed, action: ("Try again", search.retry))
                .padding(.horizontal, margin)
        }
        if !typed {
            idle.padding(.horizontal, margin)
        } else {
            if let line = SearchWords.answered(search.snapshot, query: query) {
                StateLine(symbol: "search", text: line).padding(.horizontal, margin)
            }
            if metrics.wide { wideResults } else { phoneResults }
        }
        #endif
    }

    #if !os(tvOS)
    /// No query: the hint (or that no addon can search), then Recent.
    @ViewBuilder private var idle: some View {
        VStack(alignment: .leading, spacing: metrics.wide ? 24 : 20) {
            Text(SearchWords.idle(search.snapshot))
                .textFont(metrics.wide ? 13 : 15, relativeTo: .footnote)
                .foregroundStyle(Theme.Palette.dim)
            if let recent = search.snapshot?.recent, !recent.isEmpty {
                RecentList(recent: recent, input: input)
            }
        }
    }

    /// The Mac and iPad: Top results as cards, then each group as a shelf.
    @ViewBuilder private var wideResults: some View {
        let rows = search.snapshot?.rows ?? []
        if let top = SearchWords.top(rows) {
            let columns = SearchRules.topResultColumns(width: width)
            VStack(alignment: .leading, spacing: 16) {
                RowHeader(title: SearchWords.topResults)
                switch top {
                case .loading:
                    ResultSkeleton(count: 6, wide: true, columns: columns)
                case .tiles(let tiles):
                    LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 20, alignment: .top), count: columns), alignment: .leading, spacing: 20) {
                        ForEach(tiles.keyed, id: \.key) { key, tile in
                            NavigationLink(value: TitleRoute(tile)) { TopResultCard(tile: tile) }
                                .buttonStyle(CardButtonStyle(radius: 14, scale: Theme.Focus.scaleTile))
                                .modifier(FirstResult(focus: $firstResult, first: key == tiles.keyed.first?.key))
                        }
                    }
                }
            }
            .padding(.horizontal, margin)
        }
        ForEach(SearchWords.groups(rows), id: \.rowId) { row in
            RowView(row: row, retry: { setQuery(query) })
        }
    }

    /// The iPhone: one list, Top results first, then each group with its title.
    @ViewBuilder private var phoneResults: some View {
        let rows = search.snapshot?.rows ?? []
        if let top = SearchWords.top(rows) {
            section(SearchWords.topResults) {
                switch top {
                case .loading: ResultSkeleton(count: 6, wide: false)
                case .tiles(let tiles): resultRows(tiles, first: true)
                }
            }
        }
        ForEach(SearchWords.groups(rows), id: \.rowId) { row in
            section(row.title) {
                switch row.state {
                case .loading, .slow: ResultSkeleton(count: 3, wide: false)
                case .ready: resultRows(row.tiles, first: SearchWords.top(rows) == nil && row.rowId == SearchWords.groups(rows).first?.rowId)
                case .empty: StateLine(symbol: "inbox", text: row.stateWords ?? "")
                case .failed: StateLine(symbol: "triangle-alert", text: row.stateWords ?? "", action: ("Try again", { setQuery(query) }))
                }
            }
        }
    }

    private func section<Rows: View>(_ title: String, @ViewBuilder rows: () -> Rows) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title)
                .display(20)
                .foregroundStyle(Theme.Palette.text)
                .accessibilityAddTraits(.isHeader)
            rows()
        }
        .padding(.horizontal, margin)
    }

    private func resultRows(_ tiles: [Tile], first: Bool) -> some View {
        VStack(spacing: 0) {
            ForEach(tiles.keyed, id: \.key) { key, tile in
                NavigationLink(value: TitleRoute(tile)) { SearchResultRow(tile: tile) }
                    .buttonStyle(.plain)
                    .modifier(FirstResult(focus: $firstResult, first: first && key == tiles.keyed.first?.key))
            }
        }
    }
    #endif

    #if os(tvOS)
    @ViewBuilder private var tvContent: some View {
        if search.failure != nil {
            StateLine(symbol: "triangle-alert", text: SearchWords.failed, action: ("Try again", search.retry))
                .padding(.horizontal, metrics.gutter)
        }
        if !typed {
            StateLine(symbol: "search", text: SearchWords.idle(search.snapshot)).padding(.horizontal, metrics.gutter)
        } else if let line = SearchWords.answered(search.snapshot, query: query) {
            StateLine(symbol: "search", text: line).padding(.horizontal, metrics.gutter)
        }
        ForEach(search.snapshot?.rows ?? [], id: \.rowId) { row in
            RowView(row: row, retry: { setQuery(query) })
        }
    }
    #endif
}

/// The first result takes focus on Return in a field.
private struct FirstResult: ViewModifier {
    var focus: FocusState<Bool>.Binding
    let first: Bool

    func body(content: Content) -> some View {
        if first { content.focused(focus) } else { content }
    }
}
