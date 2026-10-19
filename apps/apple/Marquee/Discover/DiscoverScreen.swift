import Foundation
import SwiftUI

/// Browse one catalog at a time: type, catalog and genre chips over a
/// poster grid that loads more as it scrolls.
struct DiscoverScreen: View {
    @State private var discover: LiveRecord<DiscoverSnapshot>
    /// First run opens the recommended addons here too.
    @State private var addons: LiveRecord<AddonsSnapshot>
    @Environment(\.metrics) private var metrics
    @Environment(SyncModel.self) private var sync
    private let core: CoreClient
    @State private var onboarding = false

    init(core: CoreClient) {
        self.core = core
        _discover = State(initialValue: LiveRecord("discover", core: core))
        _addons = State(initialValue: LiveRecord("addons", core: core))
    }

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                ScreenTitle(text: "Discover")
                if let s = discover.snapshot {
                    // Without catalogs there is nothing to filter: only the way on shows.
                    if !s.kinds.isEmpty { filters(s) }
                    content(s)
                } else if let failure = discover.failure {
                    StateLine(symbol: "triangle-alert", text: failure, action: ("Try again", discover.retry))
                }
            }
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .topScrim()
        .task(id: discover.taskId) { discover.start() }
        .task(id: addons.taskId) { addons.start() }
        .onChange(of: addons.snapshot?.fresh == true, initial: true) { _, fresh in if fresh { onboarding = true } }
        .modifier(FullScreen(isPresented: $onboarding) {
            NavigationStack {
                OnboardingScreen(core: core) { onboarding = false }
            }
            .environment(\.metrics, metrics)
            .environment(sync)
        })
    }

    @ViewBuilder private func filters(_ s: DiscoverSnapshot) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            ChipRow(
                label: "Type", chips: s.kinds.map { Chip(id: $0.id, title: $0.label) },
                isSelected: { $0 == s.kind }, select: { select(["kind": $0]) })
            if !s.catalogs.isEmpty {
                ChipRow(
                    label: "Catalog", chips: s.catalogs.map { Chip(id: $0.id, title: $0.name, detail: $0.source) },
                    isSelected: { $0 == s.catalog }, select: { select(["catalog": $0]) })
            }
            if !s.genres.isEmpty {
                // "" stands for All, which a catalog that needs a genre does not offer.
                let all = s.genreRequired ? [] : [Chip(id: "", title: "All")]
                ChipRow(
                    label: "Genre", chips: all + s.genres.map { Chip(id: $0, title: $0) },
                    isSelected: { $0 == (s.genre ?? "") },
                    select: { select(["genre": $0.isEmpty ? NSNull() : $0]) })
            }
        }
    }

    @ViewBuilder private func content(_ s: DiscoverSnapshot) -> some View {
        if s.kinds.isEmpty {
            CatalogSetup(
                title: "Nothing to browse yet",
                text: "Discover lists the catalogs of the addons you have on, and none of them has one. Set up Marquee offers the recommended addons.",
                titleSize: Theme.type.title2, core: core, setUp: { onboarding = true })
        } else if s.genreRequired && s.genre == nil {
            StateLine(symbol: "list-filter", text: "This catalog lists titles by genre. Pick one above.")
        }
        PosterGrid(tiles: s.items, onEnd: {
            if s.state == .ready { discover.command(["type": "loadMore"]) }
        })
        switch s.state {
        case .loading:
            ProgressView().controlSize(.large).frame(maxWidth: .infinity).padding(Theme.Space.l)
        case .failed(let reason):
            StateLine(symbol: "triangle-alert", text: "This catalog did not answer: \(reason)", action: ("Try again", {
                discover.command(["type": "loadMore"])
            }))
        case .end where s.items.isEmpty && !s.kinds.isEmpty:
            StateLine(symbol: "inbox", text: "Nothing in this catalog.")
        case .ready, .end:
            EmptyView()
        }
    }

    private func select(_ fields: [String: any Sendable]) {
        var command = fields
        command["type"] = "select"
        discover.command(command)
    }
}
