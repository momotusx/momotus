import Foundation
import SwiftUI

/// The user's library: type filter and sort chips over posters with progress.
struct LibraryScreen: View {
    let core: CoreClient
    @State private var library: LiveRecord<LibrarySnapshot>
    @Environment(\.metrics) private var metrics

    init(core: CoreClient) {
        self.core = core
        _library = State(initialValue: LiveRecord("library", core: core))
    }

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                HStack(alignment: .firstTextBaseline) {
                    ScreenTitle(text: "Library")
                    Spacer()
                    // A value, like the titles pushed from it: a destination link
                    // under a value push is popped when Library redraws.
                    NavigationLink(value: CalendarRoute()) {
                        Label("Calendar", systemImage: "calendar")
                    }
                    .buttonStyle(PillButtonStyle())
                }
                if let s = library.snapshot {
                    let total = s.kinds.reduce(0) { $0 + $1.count }
                    VStack(alignment: .leading, spacing: 12) {
                        ChipRow(
                            label: "Show",
                            chips: [Chip(id: "", title: "All", detail: "\(total)")]
                                + s.kinds.map { Chip(id: $0.kind, title: $0.label, detail: "\($0.count)") },
                            isSelected: { $0 == (s.kind ?? "") },
                            select: { library.command(["type": "setFilter", "kind": $0.isEmpty ? NSNull() : $0]) })
                        ChipRow(
                            label: "Sort",
                            chips: [Chip(id: "recent", title: "Last watched"), Chip(id: "added", title: "Recently added"), Chip(id: "name", title: "Name")],
                            isSelected: { $0 == s.sort },
                            select: { library.command(["type": "setSort", "sort": $0]) })
                    }
                    if s.items.isEmpty {
                        StateLine(symbol: "books.vertical", text: total == 0
                            ? "Titles you add with \u{201C}Add to Library\u{201D} show up here."
                            : "Nothing of this type in your library.")
                    }
                    PosterGrid(tiles: s.items, caption: caption)
                } else if let failure = library.failure {
                    StateLine(symbol: "exclamationmark.triangle", text: failure)
                }
            }
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .navigationDestination(for: CalendarRoute.self) { _ in CalendarScreen(core: core) }
        .task(id: library.taskId) { library.start() }
    }

    /// "Movie, 25% watched", "S2:E3, 37% watched", or the year.
    private func caption(_ tile: Tile) -> String? {
        let what = episodeLabel(tile.videoId) ?? (tile.kind == "movie" ? "Movie" : nil)
        guard let progress = tile.progress else { return tile.releaseInfo ?? what }
        return [what, "\(Int((progress * 100).rounded()))% watched"].compactMap { $0 }.joined(separator: ", ")
    }
}
