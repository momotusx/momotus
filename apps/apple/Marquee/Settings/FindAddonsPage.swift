import Foundation
import Observation

/// Find addons' page: the catalog view, and the cards' results kept or
/// forgotten as the page opens and reloads. A model, not view state, so the
/// open path runs in tests as it runs on screen.
@MainActor @Observable
final class FindAddonsPage {
    private(set) var catalog: LiveRecord<AddonCatalogSnapshot>
    @ObservationIgnored private let core: CoreClient
    @ObservationIgnored private var opened = false

    init(core: CoreClient) {
        self.core = core
        catalog = LiveRecord("addonCatalog", core: core)
    }

    func list(_ id: String) -> AddonList? { catalog.snapshot?.lists.first { $0.id == id } }

    /// Every list in the core's order: Cinemeta's two, then the installed addons'.
    var lists: [AddonList] { catalog.snapshot?.lists ?? [] }

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

    /// Try again and Refresh: the view opens again with `fresh`, which waits
    /// for the network instead of the addon cache. Results from before are
    /// about the old list; requests still running keep their cards.
    func reload() {
        InstallCards.shared.forgetAll(running: false)
        catalog = LiveRecord("addonCatalog", params: ["fresh": true], core: core)
        catalog.start()
    }
}
