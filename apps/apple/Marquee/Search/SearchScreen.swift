import SwiftUI

/// Search across every catalog the addons offer. The core debounces the
/// query; results arrive as rows, recent searches fill the empty state.
struct SearchScreen: View {
    @State private var search: LiveRecord<SearchSnapshot>
    @State private var query = ""
    @FocusState private var fieldFocused: Bool
    @Environment(\.metrics) private var metrics
    @Environment(\.tabShown) private var shown
    @Environment(\.searchRequest) private var searchRequest

    init(core: CoreClient) {
        _search = State(initialValue: LiveRecord("search", core: core))
    }

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: metrics.rowSpacing) {
                #if !os(tvOS)
                field.padding(.horizontal, metrics.gutter)
                #endif
                if let failure = search.failure {
                    StateLine(symbol: "triangle-alert", text: failure, action: ("Try again", search.retry)).padding(.horizontal, metrics.gutter)
                }
                if query.trimmingCharacters(in: .whitespaces).isEmpty {
                    recent.padding(.horizontal, metrics.gutter)
                } else if let snapshot = search.snapshot, snapshot.rows.isEmpty, snapshot.query == query {
                    StateLine(symbol: "search", text: "Nothing in your catalogs matches \u{201C}\(query)\u{201D}.")
                        .padding(.horizontal, metrics.gutter)
                }
                ForEach(search.snapshot?.rows ?? [], id: \.rowId) { row in
                    RowView(row: row, retry: { setQuery(query) })
                }
            }
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        #if os(iOS)
        .scrollDismissesKeyboard(.immediately)
        #endif
        .background(Theme.Palette.bg)
        .topScrim()
        #if os(tvOS)
        // The system's inline keyboard with the results live under it, and
        // dictation, as the TV's own apps search (review apple-journey P2-8).
        .searchable(text: $query, prompt: Self.prompt)
        .searchSuggestions {
            ForEach(search.snapshot?.recent ?? [], id: \.self) { Label($0, glyph: "history").searchCompletion($0) }
        }
        #endif
        .task(id: search.taskId) { search.start() }
        .onChange(of: query) { _, value in setQuery(value) }
        // A title opened from the results covers this page: the query goes into
        // Recent, however the tile was chosen (a tap, VoiceOver, a keyboard, a remote).
        .onDisappear { if !query.trimmingCharacters(in: .whitespaces).isEmpty { search.command(["type": "remember"]) } }
        // The keyboard opens on the first visit, not over the results on the way back from a title.
        .onAppear { if query.isEmpty { fieldFocused = true } }
        #if DEBUG
        .task {
            // After the view opened and the launch's addons installed.
            guard query.isEmpty, let typed = DebugHooks.search else { return }
            try? await Task.sleep(for: .seconds(8))
            query = typed
        }
        #endif
        // Cmd-F on the Mac: the cursor goes in the field every time.
        .onChange(of: searchRequest) { fieldFocused = true }
        // Out of sight the field lets go, so typing elsewhere never lands in it;
        // on the Mac it takes the cursor again each time Search shows.
        .onChange(of: shown) { _, now in
            #if os(macOS)
            fieldFocused = now
            #else
            if !now { fieldFocused = false }
            #endif
        }
    }

    private static let prompt = "Search titles"

    private func setQuery(_ value: String) {
        search.command(["type": "setQuery", "query": value])
    }

    private var field: some View {
        HStack(spacing: 12) {
            Glyph("search").foregroundStyle(Theme.Palette.muted)
            TextField(Self.prompt, text: $query)
                .textFieldStyle(.plain)
                .textFont(Theme.type.headline + 2, relativeTo: .title3)
                .foregroundStyle(Theme.Palette.text)
                .focused($fieldFocused)
                .autocorrectionDisabled()
                #if os(iOS)
                .textInputAutocapitalization(.never)
                .submitLabel(.search)
                #endif
            if !query.isEmpty {
                Button { query = "" } label: {
                    Glyph("circle-x").foregroundStyle(Theme.Palette.muted)
                        .frame(width: 44, height: 44)
                }
                .buttonStyle(.round)
                .accessibilityLabel("Clear search")
            }
        }
        .padding(.horizontal, 18)
        .padding(.vertical, metrics.wide ? 16 : 12)
        .frame(maxWidth: 780)
        .background(Capsule().fill(Theme.Palette.surface))
        .overlay(Capsule().strokeBorder(fieldFocused ? Theme.Palette.accent : Theme.Palette.control, lineWidth: fieldFocused ? 2 : 1))
        .modifier(FocusRing(shape: Capsule(), active: fieldFocused))
        .animation(Theme.Motion.focus, value: fieldFocused)
    }

    @ViewBuilder private var recent: some View {
        let recent = search.snapshot?.recent ?? []
        if recent.isEmpty {
            StateLine(symbol: "search", text: "Search finds titles in every catalog your addons offer.")
        } else {
            // On TV the recent searches are the keyboard's suggestions.
            #if !os(tvOS)
            VStack(alignment: .leading, spacing: 10) {
                Text(caps: "Recent")
                    .textFont(Theme.type.caption, weight: .semibold, relativeTo: .caption)
                    .tracking(1.6)
                    .foregroundStyle(Theme.Palette.muted)
                FlowRow(spacing: 10) {
                    ForEach(recent, id: \.self) { text in
                        Button { query = text } label: { Label(text, glyph: "history") }
                            .buttonStyle(ChipStyle(selected: false))
                    }
                }
            }
            #endif
        }
    }
}
