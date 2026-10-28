import Foundation
import Observation
import SwiftUI

private typealias W = FindAddonsWords

/// Find addons' page: the catalog view, and the cards' results kept or
/// forgotten as the page opens and reloads. A model, not view state, so the
/// open path runs in tests as it runs on screen.
@MainActor @Observable
final class FindAddonsPage {
    private(set) var catalog: LiveRecord<AddonCatalogSnapshot>
    /// The install sheet's question, while one is open.
    let gate = InstallGate()
    /// A card's request to the core (`installListed`, `setAddonEnabled`): the seam tests use.
    @ObservationIgnored var send: @MainActor (_ op: String, _ fields: [String: any Sendable]) async throws -> Void
    @ObservationIgnored private let core: CoreClient
    @ObservationIgnored private var opened = false

    init(core: CoreClient) {
        self.core = core
        send = { try await core.perform($0, $1) }
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

    /// A card's Install or Turn on. Install asks first with the install
    /// sheet, whose Install sends it; Turn on is sent at once (the addon is
    /// installed). A press while the card's request runs does nothing;
    /// VoiceOver hears how it ended.
    func perform(_ entry: ListedAddon, _ act: FindAction) {
        let cards = InstallCards.shared
        guard !cards[entry.key].busy else { return }
        let (key, name, send) = (entry.key, entry.name, self.send)
        let request: @MainActor () async throws -> Void
        let busy: String
        let done: String
        switch act {
        case .install:
            request = { try await send("installListed", ["key": key]) }
            busy = W.installing
            done = W.installed
        case .turnOn(let url, _):
            request = { try await send("setAddonEnabled", ["url": url, "enabled": true]) }
            // A configured copy turned on reads Set up, the listed link Installed.
            busy = W.turningOn
            done = url == key ? W.installed : W.setUp
        case .configure, .none:
            return
        }
        let run: @MainActor () -> Void = {
            // Said at once: the card's state line changes under the finger, not where VoiceOver reads.
            announce("\(name), \(busy)")
            Task {
                guard let result = await cards.run(key, request) else { return }
                announce(result.words ?? "\(name), \(done)")
            }
        }
        guard case .install = act else { return run() }
        // One question at a time: a second press while the sheet is up does not present it again.
        guard gate.asking == nil else { return }
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
