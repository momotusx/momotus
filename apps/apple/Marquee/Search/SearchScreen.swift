import SwiftUI

/// Search across every catalog the addons offer (plans/design-v3-phase7-apple.md 3). The core debounces
/// the query; Top results and the kind groups arrive as rows, recent searches fill the empty page. The
/// Mac and iPad: tv.apple.com's Top results cards over poster shelves. The iPhone: one list. TV keeps
/// its rows under the system's keyboard (D9).
struct SearchScreen: View {
    @State private var search: LiveRecord<SearchSnapshot>
    /// Browse by genre on the iPhone (C43), opened only there.
    @State private var genres: LiveRecord<GenresSnapshot>
    @Environment(\.linkRequests) private var links
    @Environment(\.openTab) private var openTab
    @FocusState private var fieldFocused: Bool
    /// Return in a field: the first result takes focus.
    @FocusState private var firstResult: Bool
    @Environment(\.metrics) private var metrics
    @Environment(\.tabShown) private var shown
    /// The query, shared with the Mac header's field (plans/design-v3-phase7-apple.md 2).
    @Environment(SearchInput.self) private var input
    @State private var width: CGFloat = 0
    /// Recommended addons, pushed from the line that says no addon can search.
    @State private var addons = false
    @Environment(ProfileSession.self) private var profiles: ProfileSession?
    private let core: CoreClient

    init(core: CoreClient) {
        self.core = core
        _search = State(initialValue: LiveRecord("search", core: core))
        _genres = State(initialValue: LiveRecord("genres", core: core))
    }

    private var query: String { input.text }
    /// A kids profile searches its own list: no addon line, no genres (plans/m11-kids.md 6.4).
    private var kids: Bool { profiles?.snapshot?.current?.kids == true }
    private var prompt: String { kids ? SearchWords.kidsPrompt : Self.prompt }
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
        .searchable(text: Bindable(input).text, prompt: prompt)
        .searchSuggestions {
            ForEach(search.snapshot?.recent ?? [], id: \.self) { Label($0, glyph: "history").searchCompletion($0) }
        }
        #endif
        .navigationDestination(isPresented: $addons) { RecommendedAddonsPage(core: core, scope: .all) }
        .task(id: search.taskId) { search.start() }
        #if os(iOS)
        .task(id: genres.taskId) { if !metrics.wide && !kids { genres.start() } }
        #endif
        .onChange(of: input.text) { _, value in setQuery(value) }
        // A query typed or linked before the view could take it (a director chip opening Search).
        .onChange(of: search.snapshot == nil) { _, none in
            if !none, !query.isEmpty, search.snapshot?.query != query { setQuery(query) }
        }
        // A title opened from the results covers this page: the query goes into
        // Recent, however the tile was chosen (a tap, VoiceOver, a keyboard, a remote).
        .onDisappear { if typed { remember() } }
        // The Mac: the cursor goes in the field on the first visit, not over the results on the way back from
        // a title. The iPhone and iPad: no keyboard until the field is tapped, so Browse by genre shows.
        .onAppear { if SearchRules.focusesOnOpen(text: query) { fieldFocused = true } }
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
                remember()
                input.text = ""
            }
        }
        // A link asking for the query already in the field: asked again.
        .onChange(of: input.requery) { setQuery(query) }
    }

    private static let prompt = "Search titles"

    /// Keeps the field's text in Recent: the text itself, not the core's query, which lands after its debounce,
    /// so a result opened or a place left right after the last keystrokes keeps them (`remember {query}`).
    private func remember() {
        search.command(SearchRules.rememberCommand(text: query))
    }

    private func setQuery(_ value: String) {
        search.command(["type": "setQuery", "query": value])
    }

    /// No addon can search: the line's way on is one that does (the web's and Android's Recommended addons).
    private var addonsAction: (title: String, run: () -> Void)? {
        SearchWords.offersAddons(search.snapshot, kids: kids) ? (SearchWords.addons, { addons = true }) : nil
    }

    /// The page's edge (the token's on the phone): the field, Recent, the lists and the genre tiles sit on it.
    private var margin: CGFloat { metrics.gutter }

    @ViewBuilder private var content: some View {
        #if os(tvOS)
        tvContent
        #else
        if !metrics.wide { ScreenTitle(text: "Search").padding(.horizontal, margin) }
        SearchField(input: input, prompt: prompt, focused: $fieldFocused).padding(.horizontal, margin)
        if search.failure != nil {
            // The core's words can name a host or an addon: the shared line instead.
            StateLine(symbol: "triangle-alert", text: SearchWords.failed, action: ("Try again", search.retry), spoken: true)
                .padding(.horizontal, margin)
        }
        if !typed {
            idle.padding(.horizontal, margin)
        } else {
            if let line = SearchWords.answered(search.snapshot, query: query, kids: kids) {
                StateLine(symbol: "search", text: line, action: addonsAction, spoken: true).padding(.horizontal, margin)
            }
            if metrics.wide { wideResults } else { phoneResults }
        }
        #endif
    }

    #if !os(tvOS)
    /// No query: the hint (or that no addon can search), then Recent.
    @ViewBuilder private var idle: some View {
        VStack(alignment: .leading, spacing: metrics.wide ? 24 : 20) {
            Text(SearchWords.idle(search.snapshot, kids: kids))
                .textFont(metrics.wide ? 13 : 15, relativeTo: .footnote)
                .foregroundStyle(Theme.Palette.dim)
            if let addonsAction {
                Button(addonsAction.title, action: addonsAction.run).buttonStyle(PillButtonStyle())
            }
            if let recent = search.snapshot?.recent, !recent.isEmpty {
                RecentList(recent: recent, input: input)
            }
            #if os(iOS)
            // The phone's genre tiles; a failed grid is not drawn (Search's own words cover the page).
            if !metrics.wide, !kids, genres.failure == nil {
                GenreGrid(snapshot: genres.snapshot) { kind, genre in
                    // The genre chip's path: one select of the kind and the genre (C40).
                    links?.discover = DiscoverRequest(kind: kind, genre: genre)
                    openTab(.discover, link: true)
                }
            }
            #endif
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
            StateLine(symbol: "triangle-alert", text: SearchWords.failed, action: ("Try again", search.retry), spoken: true)
                .padding(.horizontal, metrics.gutter)
        }
        if !typed {
            StateLine(symbol: "search", text: SearchWords.idle(search.snapshot, kids: kids), action: addonsAction).padding(.horizontal, metrics.gutter)
        } else if let line = SearchWords.answered(search.snapshot, query: query, kids: kids) {
            StateLine(symbol: "search", text: line, action: addonsAction, spoken: true).padding(.horizontal, metrics.gutter)
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
