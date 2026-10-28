import Foundation
import Observation

/// Find addons' page: the catalog view, and the cards' results kept or
/// forgotten as the page opens and reloads. A model, not view state, so the
/// open path runs in tests as it runs on screen.
@MainActor @Observable
final class FindAddonsPage {
    private(set) var catalog: LiveRecord<AddonCatalogSnapshot>
    /// The install sheet's question, while one is open.
    let gate = InstallGate()
    @ObservationIgnored private let core: CoreClient
    @ObservationIgnored private var opened = false

    init(core: CoreClient) {
        self.core = core
        catalog = LiveRecord("addonCatalog", core: core)
    }

    func list(_ id: String) -> AddonList? { catalog.snapshot?.lists.first { $0.id == id } }

    /// Every list in the core's order: Cinemeta's two, then the installed addons'.
    var lists: [AddonList] { catalog.snapshot?.lists ?? [] }

    /// Each entry's folded search text by key, with the text it was folded
    /// from: made once per entry, not per keystroke or render, and again when
    /// a refreshed list changed that entry's name, host or description.
    @ObservationIgnored private var searchTexts: [String: (source: String, folded: String)] = [:]

    func searchText(of entry: ListedAddon) -> String {
        let source = "\(entry.name)\u{0}\(entry.host)\u{0}\(entry.description ?? "")"
        if let kept = searchTexts[entry.key], kept.source == source { return kept.folded }
        let folded = ListedAddon.searchText(entry)
        // The cache never outgrows the lists the page has shown.
        if searchTexts.count > 10_000 { searchTexts.removeAll() }
        searchTexts[entry.key] = (source, folded)
        return folded
    }

    func status(of id: String) -> FindStatus { FindStatus(failure: catalog.failure, list: list(id)) }

    /// The page appeared. The first time, results from an earlier visit go:
    /// they are about what it showed then. Requests still running keep their
    /// cards; their answers land on them. Later appearances (back from a
    /// page on top) keep everything.
    func appear() {
        guard !opened else { return }
        opened = true
        InstallCards.shared.forgetAll(running: false)
    }

    /// A card's press: Install asks first with the install sheet, whose
    /// Install runs `run`; Turn on runs at once (the addon is installed).
    func press(_ entry: ListedAddon, _ act: FindAction, run: @escaping @MainActor () -> Void) {
        guard case .install = act else { return run() }
        gate.ask(InstallSheetRules.ask(name: entry.name, host: entry.host, provides: entry.provides, types: entry.types, run: run))
    }

    /// Try again and Refresh: the view opens again with `fresh`, which waits
    /// for the network instead of the addon cache. Results from before are
    /// about the old list; requests still running keep their cards.
    func reload() {
        InstallCards.shared.forgetAll(running: false)
        catalog = LiveRecord("addonCatalog", params: ["fresh": true], core: core)
        catalog.start()
    }
}
