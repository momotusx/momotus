import Foundation
import Testing

// Find addons (plans/m12-addon-discovery.md): the words against the shared
// file, a recorded snapshot, the card rules (the web's lib/find-addons.ts)
// and the card states (lib/install-cards.ts).

private typealias W = FindAddonsWords

private func entry(
    _ name: String = "OpenSubtitles v3", installed: ListedAddon.Installed = .absent,
    configuration: ListedAddon.Configuration = .none, copyKey: String? = nil,
    provides: [String] = ["subtitles"], types: [String] = ["movie", "series"], host: String = "opensubtitles-v3.strem.io"
) -> ListedAddon {
    ListedAddon(
        key: "https://\(host)/manifest.json", id: "org.example", host: host, name: name, description: nil,
        types: types, provides: provides, configuration: configuration,
        configureUrl: "https://\(host)/configure", installed: installed, copyKey: copyKey)
}

/// A refusal as the core sends it: a code and the words for the card.
private struct Refused: Error {
    let code: String
    let words: String
}

private func refusal(_ error: any Error) -> (code: String?, words: String) {
    let refused = error as? Refused
    return (refused?.code, refused?.words ?? "unknown")
}

/// A request that waits until the test answers it.
@MainActor private final class Held {
    private var continuation: CheckedContinuation<Void, any Error>?
    private(set) var started = 0

    func request() async throws {
        started += 1
        try await withCheckedThrowingContinuation { continuation = $0 }
    }

    func answer(_ result: Result<Void, any Error>) {
        continuation?.resume(with: result)
        continuation = nil
    }

    func waitForStart(_ count: Int = 1) async {
        while started < count { await Task.yield() }
    }
}

@Suite struct FindAddonsTests {
    @Test func theWordsAreTheSharedOnes() throws {
        let url = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../../shared/find-addons-words.json")
        let shared = try JSONDecoder().decode([String: String].self, from: Data(contentsOf: url))
        #expect(shared.count == 34)
        #expect(W.table == shared)
        for (key, value) in shared { #expect(W.table[key] == value, "\(key)") }
    }

    @Test func aRecordedSnapshotDecodes() throws {
        let url = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "Fixtures/addon-catalog.json")
        let snapshot = try JSONDecoder().decode(AddonCatalogSnapshot.self, from: Data(contentsOf: url))
        #expect(snapshot.lists.map(\.id) == ["official", "community"])
        #expect(snapshot.lists.map(\.name) == ["Official", "Community"])
        #expect(snapshot.lists.allSatisfy { $0.state == .ready })
        let official = snapshot.lists[0].entries
        #expect(official.map(\.name) == ["Cinemeta", "YouTube", "WatchHub", "Public Domain Movies", "OpenSubtitles v3"])
        #expect(official[0].installed == .on)
        #expect(official[2].installed == .absent && official[2].provides == ["stream"])
        #expect(official.allSatisfy { $0.configureUrl.hasSuffix("/configure") && $0.copyKey == nil })
        let community = snapshot.lists[1].entries
        #expect(community.contains { $0.configuration == .required })
        #expect(community.contains { $0.configuration == .optional })
        #expect(community.contains { $0.name.readsRightToLeft })
    }

    @Test func listStatesAndCopiesDecode() throws {
        let json = #"""
        {"lists": [{"id": "official", "name": "Official", "state": {"state": "failed", "words": "Cinemeta did not answer."}, "entries": []},
                   {"id": "community", "name": "Community", "state": {"state": "loading"}, "entries": [
                     {"key": "https://a.example/x/manifest.json", "id": "a", "host": "a.example", "name": "A", "description": null,
                      "types": [], "provides": ["catalog"], "configuration": "optional", "configureUrl": "https://a.example/x/configure",
                      "installed": "copyOff", "copyKey": "https://a.example/y/manifest.json"}]}]}
        """#
        let snapshot = try JSONDecoder().decode(AddonCatalogSnapshot.self, from: Data(json.utf8))
        #expect(snapshot.lists[0].state == .failed(words: "Cinemeta did not answer."))
        #expect(snapshot.lists[1].state == .loading)
        #expect(snapshot.lists[1].entries[0].installed == .copyOff)
        #expect(snapshot.lists[1].entries[0].copyKey == "https://a.example/y/manifest.json")
        let unknown = json.replacingOccurrences(of: #""loading""#, with: #""later""#)
        #expect(throws: DecodingError.self) { try JSONDecoder().decode(AddonCatalogSnapshot.self, from: Data(unknown.utf8)) }
    }

    @Test func theActionFollowsTheInstalledStateFirst() {
        // Installed or set up: words, no action, whatever the configuration.
        #expect(entry(installed: .on, configuration: .required).action == .none(word: "Installed"))
        #expect(entry(installed: .copyOn, configuration: .required).action == .none(word: "Set up"))
        // Off: turn this link on; a set-up copy off: turn that copy on.
        let off = entry(installed: .off, configuration: .required)
        #expect(off.action == .turnOn(url: off.key, word: "Off"))
        #expect(entry(installed: .copyOff, configuration: .required, copyKey: "https://copy.example/manifest.json").action
            == .turnOn(url: "https://copy.example/manifest.json", word: "Set up, off"))
        #expect(entry(installed: .copyOff, copyKey: nil).action == .none(word: "Set up, off"))
        // Absent: the configuration decides.
        #expect(entry(configuration: .required).action == .configure)
        #expect(entry(configuration: .optional).action == .install(configure: true))
        #expect(entry(configuration: .none).action == .install(configure: false))
    }

    @Test func aRefusalForSetupAsksForConfigureUntilInstalledAnotherWay() {
        #expect(entry().cardAction(lastCode: "needsSetup") == .configure)
        #expect(entry().cardAction(lastCode: "timeout") == .install(configure: false))
        #expect(entry().cardAction(lastCode: nil) == .install(configure: false))
        #expect(entry(installed: .copyOn).cardAction(lastCode: "needsSetup") == .none(word: "Set up"))
    }

    @Test func whatAnActionShowsAndOffers() {
        #expect(FindAction.install(configure: true).offersConfigure)
        #expect(FindAction.configure.offersConfigure)
        #expect(!FindAction.install(configure: false).offersConfigure)
        #expect(FindAction.install(configure: false).pressable && FindAction.turnOn(url: "u", word: "Off").pressable)
        #expect(!FindAction.configure.pressable && !FindAction.none(word: "Installed").pressable)
        #expect(FindAction.turnOn(url: "u", word: "Off").word == "Off")
        #expect(FindAction.install(configure: true).word == nil)
    }

    @Test func theProvidesLineNamesResourcesAndKinds() {
        #expect(entry().providesLine == "Subtitles for movies and series")
        #expect(entry(provides: ["catalog", "meta", "addon_catalog"], types: ["movie", "series", "channel"]).providesLine
            == "Catalogs, Details, Addon lists for movies, series and channels")
        #expect(entry(provides: ["stream"], types: ["tv"]).providesLine == "Streams for live TV")
        #expect(entry(provides: ["meta"], types: []).providesLine == "Details")
        #expect(entry(provides: ["catalog"], types: ["Dice"]).providesLine == "Catalogs for dice")
    }

    @Test func repeatedNamesAreThoseOnTwoCards() {
        let entries = [entry("Trakt Tv"), entry("Cinemeta"), entry("Trakt Tv", host: "trakt.example"), entry("trakt tv")]
        #expect(ListedAddon.repeatedNames(entries) == ["Trakt Tv"])
        #expect(ListedAddon.repeatedNames([]).isEmpty)
    }

    @Test func theInitialSkipsSymbols() {
        #expect(entry("🕒 Clockrr (Top Right)").initial == "C")
        #expect(entry("[NEW] TurkceAltyazi.Org").initial == "N")
        #expect(entry("кючек").initial == "К")
        #expect(entry("כתוביות בעברית").initial == "כ")
        #expect(entry("4K Catalog").initial == "4")
        #expect(entry("***").initial == "")
    }

    @Test func aCardIsSpokenAsItsActionAndName() {
        let subs = entry()
        #expect(subs.spoken(subs.action, sameName: false) == "Install OpenSubtitles v3, subtitles")
        let trakt = entry("Trakt Tv", provides: ["catalog"], host: "trakt.example")
        #expect(trakt.spoken(trakt.action, sameName: true) == "Install Trakt Tv, from trakt.example, catalogs")
        #expect(trakt.spoken(.configure, sameName: false) == "Configure Trakt Tv, catalogs")
        #expect(trakt.spoken(.turnOn(url: trakt.key, word: "Off"), sameName: false) == "Turn on Trakt Tv, catalogs")
        // Installed: its state, not an action.
        #expect(trakt.spoken(.none(word: "Installed"), sameName: false) == "Trakt Tv, catalogs, installed")
        #expect(entry("Bare", provides: []).spoken(.none(word: ""), sameName: false) == "Bare")
    }

    @Test func filtersShowWhatAnEntryProvides() {
        let both = entry(provides: ["stream", "subtitles"])
        #expect(FindFilter.allCases.map(\.label) == ["All", "Sources", "Subtitles", "Catalogs"])
        #expect(FindFilter.all.shows(entry(provides: [])))
        #expect(FindFilter.sources.shows(both) && FindFilter.subtitles.shows(both) && !FindFilter.catalogs.shows(both))
        #expect(FindFilter.catalogs.shows(entry(provides: ["catalog"])))
        #expect(!FindFilter.sources.shows(entry(provides: ["meta"])))
        #expect(FindFilter.all.emptyWords == nil)
        #expect(FindFilter.sources.emptyWords == "No source addons in this list.")
        #expect(FindFilter.subtitles.emptyWords == "No subtitle addons in this list.")
        #expect(FindFilter.catalogs.emptyWords == "No catalog addons in this list.")
    }

    @Test func namesReadInTheirOwnDirection() {
        #expect("כתוביות בעברית".readsRightToLeft)
        #expect("ترجمة".readsRightToLeft)
        #expect("123 כתוביות".readsRightToLeft)
        #expect(!"🕒 Clockrr".readsRightToLeft)
        #expect(!"КЮЧЕК".readsRightToLeft)
        #expect(!"OpenSubtitles עברית".readsRightToLeft)
        #expect(!"".readsRightToLeft)
    }

    @MainActor @Test func aSecondPressWhileBusyDoesNothingAndTheResultLandsOnItsCard() async {
        let cards = InstallCards()
        let held = Held()
        let run = Task { await cards.run("a", held.request, failure: refusal) }
        await held.waitForStart()
        #expect(cards["a"].busy)
        await cards.run("a", held.request, failure: refusal)
        #expect(held.started == 1)
        held.answer(.failure(Refused(code: "needsSetup", words: "This addon now needs setup first")))
        await run.value
        #expect(cards["a"] == InstallCards.Card(busy: false, code: "needsSetup", words: "This addon now needs setup first"))
        #expect(cards["b"] == InstallCards.Card())
        // A success clears the card.
        let again = Task { await cards.run("a", held.request, failure: refusal) }
        await held.waitForStart(2)
        held.answer(.success(()))
        await again.value
        #expect(cards["a"] == InstallCards.Card())
    }

    @MainActor @Test func aResultFromBeforeANewProfileLandsNowhere() async {
        let cards = InstallCards()
        let held = Held()
        let run = Task { await cards.run("a", held.request, failure: refusal) }
        await held.waitForStart()
        cards.forgetAll()
        #expect(cards["a"] == InstallCards.Card())
        held.answer(.failure(Refused(code: "timeout", words: "This addon's server did not answer in time")))
        await run.value
        #expect(cards["a"] == InstallCards.Card())
    }

    @MainActor @Test func tryAgainForgetsResultsButKeepsRunningCards() async {
        let cards = InstallCards()
        let held = Held()
        let failed = Task { await cards.run("done", held.request, failure: refusal) }
        await held.waitForStart()
        held.answer(.failure(Refused(code: "unreachable", words: "This addon's server could not be reached")))
        await failed.value
        let run = Task { await cards.run("a", held.request, failure: refusal) }
        await held.waitForStart(2)
        cards.forgetAll(running: false)
        #expect(cards["done"] == InstallCards.Card())
        #expect(cards["a"].busy)
        held.answer(.failure(Refused(code: "changed", words: "Another device changed your addons first")))
        await run.value
        #expect(cards["a"].words == "Another device changed your addons first")
        #expect(cards["a"].code == "changed")
    }

    @MainActor @Test func aNewInstalledStateForgetsOnlyAFinishedResult() async {
        let cards = InstallCards()
        let held = Held()
        let run = Task { await cards.run("a", held.request, failure: refusal) }
        await held.waitForStart()
        cards.forget("a")
        #expect(cards["a"].busy)
        held.answer(.failure(Refused(code: "mismatch", words: "This addon is not the one the list named")))
        await run.value
        cards.forget("a")
        #expect(cards["a"] == InstallCards.Card())
    }
}
