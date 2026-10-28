import Foundation
import SwiftUI

/// The user's library: type filter and sort chips over posters with progress.
struct LibraryScreen: View {
    let core: CoreClient
    @State private var library: LiveRecord<LibrarySnapshot>
    @Environment(\.metrics) private var metrics
    #if !os(tvOS)
    @Environment(DownloadKeeper.self) private var keeper: DownloadKeeper?
    @Environment(\.linkRequests) private var links
    @State private var showDownloads = false
    #endif

    init(core: CoreClient) {
        self.core = core
        _library = State(initialValue: LiveRecord("library", core: core))
    }

    /// The filter and sort to put back after Try again reopened the view (nil when it was at its defaults).
    @State private var wanted: LibraryChoice?

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                ScreenTitle(text: "Library")
                #if !os(tvOS)
                // Downloads first (plans/offline-downloads.md 3.4), then the iPhone's Calendar.
                if let keeper, DownloadGate.shows(tv: DownloadGate.tv) {
                    DownloadsRow(keeper: keeper) { showDownloads = true }
                        .frame(maxWidth: metrics.wide ? 480 : .infinity, alignment: .leading)
                }
                #endif
                #if os(iOS)
                CalendarRow()
                #endif
                if let failure = library.stage.failure {
                    StateLine(symbol: "triangle-alert", text: failure, action: ("Try again", tryAgain), spoken: true)
                }
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
                            select: { library.command(LibraryChange.filter($0.isEmpty ? nil : $0).fields) })
                        ChipRow(
                            label: "Sort",
                            chips: [Chip(id: "recent", title: "Last watched"), Chip(id: "added", title: "Recently added"), Chip(id: "name", title: "Name")],
                            isSelected: { $0 == s.sort },
                            select: { library.command(LibraryChange.sort($0).fields) })
                    }
                    if s.items.isEmpty {
                        StateLine(symbol: "library-big", text: "Nothing of this type in your library.")
                    }
                    PosterGrid(tiles: s.items, caption: caption)
                } else if library.stage == .loading {
                    PosterGridSkeleton()
                }
            }
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .onChange(of: library.snapshot) { _, now in restore(now) }
        .topScrim()
        .task(id: library.taskId) { library.start() }
        #if !os(tvOS)
        .navigationDestination(isPresented: $showDownloads) {
            if let keeper { DownloadsScreen(keeper: keeper, core: core) }
        }
        // The offline banner: Downloads, once Library can show it.
        .onChange(of: links?.library, initial: true) { _, route in
            guard route == .downloads else { return }
            links?.library = nil
            showDownloads = true
        }
        #if DEBUG
        // `-MQOpenDownloads YES` (with `-MQTab library`): a Mac walk's shot of Downloads with no input sent.
        .task { if DebugHooks.openDownloads { showDownloads = true } }
        #endif
        #endif
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

    /// The core keeps the filter and sort per open view, so a reopen would drop them: they are put back
    /// when the reopened view answers.
    private func tryAgain() {
        let now = library.snapshot?.choice
        wanted = now == .fresh ? nil : now
        library.retry()
    }

    private func restore(_ snapshot: LibrarySnapshot?) {
        guard let wanted, let snapshot else { return }
        self.wanted = nil
        for change in libraryRestore(wanted: wanted, now: snapshot.choice) { library.command(change.fields) }
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
                .textFont(Theme.type.body)
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

#if os(iOS)
/// The iPhone's way to the Calendar (D6): a row at the top of Library, as an iOS grouped row.
private struct CalendarRow: View {
    @Environment(\.openTab) private var openTab

    var body: some View {
        Button { openTab(.calendar) } label: {
            HStack(spacing: 13) {
                Glyph("calendar", size: 20)
                Text("Calendar").textFont(Theme.type.body)
                Spacer(minLength: 0)
                Glyph("chevron-right", size: 14).foregroundStyle(Theme.Palette.dim)
            }
            .foregroundStyle(Theme.Palette.text)
            .padding(.horizontal, 16)
            .frame(minHeight: 51)
            .background(Theme.Palette.surface, in: RoundedRectangle(cornerRadius: 26, style: .continuous))
        }
        .buttonStyle(FocusPlainStyle(shape: RoundedRectangle(cornerRadius: 26, style: .continuous)))
        .accessibilityHint("Shows what airs next")
    }
}
#endif
