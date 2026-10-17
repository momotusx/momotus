import SwiftUI

/// Search across every catalog the addons offer. The core debounces the
/// query; results arrive as rows, recent searches fill the empty state.
struct SearchScreen: View {
    @State private var search: LiveRecord<SearchSnapshot>
    @State private var query = ""
    @FocusState private var fieldFocused: Bool
    @Environment(\.metrics) private var metrics

    init(core: CoreClient) {
        _search = State(initialValue: LiveRecord("search", core: core))
    }

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: metrics.rowSpacing) {
                field.padding(.horizontal, metrics.gutter)
                if let failure = search.failure {
                    StateLine(symbol: "triangle-alert", text: failure).padding(.horizontal, metrics.gutter)
                }
                if query.trimmingCharacters(in: .whitespaces).isEmpty {
                    recent.padding(.horizontal, metrics.gutter)
                } else if let snapshot = search.snapshot, snapshot.rows.isEmpty, snapshot.query == query {
                    StateLine(symbol: "search", text: "Nothing in your catalogs matches \u{201C}\(query)\u{201D}.")
                        .padding(.horizontal, metrics.gutter)
                }
                ForEach(search.snapshot?.rows ?? [], id: \.rowId) { row in
                    RowView(row: row, retry: { setQuery(query) }, onOpen: { search.command(["type": "remember"]) })
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
        .task(id: search.taskId) { search.start() }
        .onChange(of: query) { _, value in setQuery(value) }
        .onAppear { fieldFocused = true }
    }

    private func setQuery(_ value: String) {
        search.command(["type": "setQuery", "query": value])
    }

    private var field: some View {
        HStack(spacing: 12) {
            Glyph("search").foregroundStyle(Theme.Palette.muted)
            TextField("Movies, series, people", text: $query)
                .textFieldStyle(.plain)
                .font(.text(Theme.type.headline + 2, relativeTo: .title3))
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
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Clear search")
            }
        }
        .padding(.horizontal, 18)
        .padding(.vertical, metrics.wide ? 16 : 12)
        .frame(maxWidth: 780)
        .background(Capsule().fill(Theme.Palette.surface))
        .overlay(Capsule().strokeBorder(fieldFocused ? Theme.Palette.accent : Theme.Palette.control, lineWidth: fieldFocused ? 2 : 1))
        .animation(Theme.Motion.focus, value: fieldFocused)
    }

    @ViewBuilder private var recent: some View {
        let recent = search.snapshot?.recent ?? []
        if recent.isEmpty {
            StateLine(symbol: "search", text: "Search finds titles in every catalog your addons offer.")
        } else {
            VStack(alignment: .leading, spacing: 10) {
                Text("RECENT")
                    .font(.text(Theme.type.caption, weight: .semibold, relativeTo: .caption))
                    .tracking(1.6)
                    .foregroundStyle(Theme.Palette.muted)
                FlowRow(spacing: 10) {
                    ForEach(recent, id: \.self) { text in
                        Button { query = text } label: { Label(text, glyph: "history") }
                            .buttonStyle(ChipStyle(selected: false))
                    }
                }
            }
        }
    }
}
