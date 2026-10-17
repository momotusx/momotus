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
                ScreenTitle(text: "Library")
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
                        StateLine(symbol: "library-big", text: total == 0
                            ? "Titles you add with \u{201C}Add to Library\u{201D} show up here."
                            : "Nothing of this type in your library.")
                    }
                    PosterGrid(tiles: s.items, caption: caption)
                } else if let failure = library.failure {
                    StateLine(symbol: "triangle-alert", text: failure)
                }
            }
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .topScrim()
        .task(id: library.taskId) { library.start() }
    }

    /// The core's caption when it sends one, else the episode ("S2:E3") or the
    /// year; never a percentage (the bar on the poster shows progress).
    private func caption(_ tile: Tile) -> String? {
        tile.releaseInfo ?? episodeLabel(tile.videoId) ?? (tile.kind == "movie" ? "Movie" : nil)
    }
}
