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
                if let s = library.snapshot, total(s) == 0 {
                    // Nothing to filter or sort: the way in instead of chips that say "All 0".
                    EmptyLibrary()
                } else if let s = library.snapshot {
                    let total = total(s)
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
                        StateLine(symbol: "library-big", text: "Nothing of this type in your library.")
                    }
                    PosterGrid(tiles: s.items, caption: caption)
                } else if let failure = library.failure {
                    StateLine(symbol: "triangle-alert", text: failure, action: ("Try again", library.retry))
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

    /// Titles in the library, every kind together.
    private func total(_ s: LibrarySnapshot) -> Int {
        s.kinds.reduce(0) { $0 + $1.count }
    }
}

/// An empty library: what it holds and the way to fill it, centered.
private struct EmptyLibrary: View {
    @Environment(\.openTab) private var openTab
    @Environment(\.metrics) private var metrics

    var body: some View {
        VStack(spacing: Theme.Space.s) {
            Glyph("library-big", size: metrics.wide ? 48 : 40)
                .foregroundStyle(Theme.Palette.muted)
            Text("Your library is empty")
                .display(Theme.type.title2)
                .foregroundStyle(Theme.Palette.text)
            Text("Add titles with Add to Library on a title page, or import your Stremio library from Settings.")
                .font(.text(Theme.type.body))
                .foregroundStyle(Theme.Palette.muted)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 420)
            Button { openTab(.discover) } label: { Label("Discover", glyph: "compass") }
                .buttonStyle(PillButtonStyle(prominent: true))
                .padding(.top, Theme.Space.xs)
        }
        .frame(maxWidth: .infinity)
        .padding(.top, metrics.wide ? Theme.Space.xxl : Theme.Space.xl)
    }
}
