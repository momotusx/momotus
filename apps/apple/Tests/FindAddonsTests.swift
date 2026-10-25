import Foundation
import Testing

// Find addons (plans/m12-addon-discovery.md): the words against the shared
// file, a recorded snapshot, the card rules (the web's lib/find-addons.ts)
// and the card states (lib/install-cards.ts).

private typealias W = FindAddonsWords

private func entry(
    _ name: String = "OpenSubtitles v3", installed: ListedAddon.Installed = .absent,
    configuration: ListedAddon.Configuration = .none, copyKey: String? = nil,
    provides: [String] = ["subtitles"], types: [String] = ["movie", "series"], host: String = "opensubtitles-v3.strem.io",
    key: String? = nil, configureUrl: String? = nil
) -> ListedAddon {
    ListedAddon(
        key: key ?? "https://\(host)/manifest.json", id: "org.example", host: host, name: name, description: nil,
        types: types, provides: provides, configuration: configuration,
        configureUrl: configureUrl ?? "https://\(host)/configure", installed: installed, copyKey: copyKey)
}

private func recorded() throws -> AddonCatalogSnapshot {
    let url = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "Fixtures/addon-catalog.json")
    return try JSONDecoder().decode(AddonCatalogSnapshot.self, from: Data(contentsOf: url))
}

/// Requests that wait until the test answers them (all at once, so a
/// second request that should not have started never hangs the test).
@MainActor private final class Held {
    private var waiting: [CheckedContinuation<Void, any Error>] = []
    private(set) var started = 0

    func request() async throws {
        started += 1
        try await withCheckedThrowingContinuation { waiting.append($0) }
    }

    func answer(_ result: Result<Void, any Error>) {
        for continuation in waiting { continuation.resume(with: result) }
        waiting = []
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

    @Test func aRecordedSnapshotDecodesEveryInstalledState() throws {
        // Recorded from a bridge on an empty profile: OpenSubtitles v3 installed,
        // YouTube installed and turned off, a copy of WatchHub (its link plus a
        // query) on, a copy of Public Domain Movies off.
        let snapshot = try recorded()
        #expect(snapshot.lists.map(\.id) == ["official", "community"])
        #expect(snapshot.lists.map(\.name) == ["Official", "Community"])
        #expect(snapshot.lists.allSatisfy { $0.state == .ready })
        let official = snapshot.lists[0].entries
        #expect(official.map(\.name) == ["Cinemeta", "YouTube", "WatchHub", "Public Domain Movies", "OpenSubtitles v3"])
        #expect(official.map(\.installed) == [.absent, .off, .copyOn, .copyOff, .on])
        #expect(official[3].copyKey == official[3].key + "?copy=1")
        #expect(official.filter { $0.copyKey != nil }.count == 1)
        #expect(official.allSatisfy { $0.configureUrl.hasSuffix("/configure") })
        #expect(official[1].action == .turnOn(url: official[1].key, word: "Off"))
        #expect(official[3].action == .turnOn(url: official[3].key + "?copy=1", word: "Set up, off"))
        let community = snapshot.lists[1].entries
        #expect(community.contains { $0.configuration == .required })
        #expect(community.contains { $0.configuration == .optional })
        // Two entries of the live list share a manifest id; cards are told apart by key.
        let shared = community.filter { $0.id == "community.opensubtitles" }
        #expect(shared.count == 2)
        #expect(Set(shared.map(\.key)).count == 2)
        #expect(Set(community.map(\.key)).count == community.count)
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
        // Installed but off since: Turn on wins over the old refusal.
        let off = entry(installed: .off)
        #expect(off.cardAction(lastCode: "needsSetup") == .turnOn(url: off.key, word: "Off"))
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

    @Test func onlyHttpsConfigurePagesOpen() {
        #expect(entry(configureUrl: "https://a.example/configure").configurePage?.absoluteString == "https://a.example/configure")
        #expect(entry(configureUrl: "http://a.example/configure").configurePage == nil)
        #expect(entry(configureUrl: "stremio://a.example/configure").configurePage == nil)
        #expect(entry(configureUrl: "javascript:alert(1)").configurePage == nil)
        #expect(entry(configureUrl: "HTTPS://a.example/configure").configurePage != nil)
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

    @Test func aCardThatNeedsSetupSaysHowOnEachDevice() {
        let plexio = entry("Plexio", configuration: .required, provides: ["stream"])
        #expect(plexio.cardLabel(.configure, sameName: false, tv: false)
            == "Configure Plexio, streams. Set it up on its website, copy the link it gives you, then paste it here.")
        #expect(plexio.cardLabel(.configure, sameName: false, tv: true)
            == "Configure Plexio, streams. Set it up on its website, then add the link in Momotus on your phone or computer. With sync on, it follows you here.")
        #expect(plexio.cardLabel(.install(configure: false), sameName: false, tv: true) == "Install Plexio, streams")
        #expect(plexio.cardLabel(.none(word: "Installed"), sameName: false, tv: false) == "Plexio, streams, installed")
    }

    @Test func noKeyIsEverSpokenOrShown() {
        // Every link of an entry may hold someone's keys: none of the words a card shows or says carries them.
        let secret = "SECRET-7f3a"
        let installs: [ListedAddon.Installed] = [.absent, .on, .off, .copyOn, .copyOff]
        let configurations: [ListedAddon.Configuration] = [.none, .optional, .required]
        for installed in installs {
            for configuration in configurations {
                for code in [nil, "needsSetup", "timeout"] {
                    let card = entry(
                        "Name", installed: installed, configuration: configuration,
                        copyKey: "https://copy.example/\(secret)/manifest.json", host: "addon.example",
                        key: "https://addon.example/\(secret)/manifest.json", configureUrl: "https://addon.example/\(secret)/configure")
                    let act = card.cardAction(lastCode: code)
                    var words = [card.providesLine, card.initial, act.word ?? ""]
                    for tv in [false, true] {
                        for same in [false, true] {
                            words.append(card.cardLabel(act, sameName: same, tv: tv))
                            words.append(card.spoken(act, sameName: same))
                        }
                    }
                    #expect(!words.contains { $0.contains(secret) }, "\(installed) \(configuration) \(code ?? "-")")
                }
            }
        }
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

    @Test func aRefusalKeepsTheCoresCodeAndWords() {
        let refused = InstallCards.refusal(CoreError("This addon now needs setup first", code: "needsSetup"))
        #expect(refused.code == "needsSetup")
        #expect(refused.words == "This addon now needs setup first")
        // A code the app does not know shows its words all the same.
        #expect(InstallCards.refusal(CoreError("Something new", code: "later")) == ("later", "Something new"))
        #expect(InstallCards.refusal(CoreError("No code")).code == nil)
    }

    @Test func aViewThatDidNotOpenIsSaidAsAListThatCameIn() {
        let ready = AddonList(id: "official", name: "Official", state: .ready, entries: [entry(), entry("YouTube")])
        let loading = AddonList(id: "community", name: "Community", state: .loading, entries: [])
        // The view's own failure wins over the list's state, before or after a snapshot.
        #expect(FindStatus(failure: "The core could not open", list: nil) == .failed("The core could not open"))
        #expect(FindStatus(failure: "The core stopped", list: ready) == .failed("The core stopped"))
        #expect(FindStatus(failure: nil, list: nil) == .loading)
        #expect(FindStatus(failure: nil, list: loading) == .loading)
        #expect(FindStatus(failure: nil, list: ready) == .ready(name: "Official", count: 2))
        #expect(FindStatus(failure: nil, list: AddonList(id: "x", name: "X", state: .failed(words: ""), entries: [])) == .failed(W.failed))
        // Said: a list that came in, and every failure; not a switch between lists already in.
        #expect(FindStatus.failed("The core could not open").announcement(after: .loading) == "The core could not open")
        #expect(FindStatus.failed("The core stopped").announcement(after: .ready(name: "Official", count: 2)) == "The core stopped")
        #expect(FindStatus.ready(name: "Official", count: 2).announcement(after: .loading) == "Official, 2")
        #expect(FindStatus.ready(name: "Community", count: 9).announcement(after: .ready(name: "Official", count: 2)) == nil)
        #expect(FindStatus.loading.announcement(after: .failed("x")) == nil)
    }

    @MainActor @Test func aSecondPressWhileBusyDoesNothingAndTheResultLandsOnItsCard() async {
        let cards = InstallCards()
        let held = Held()
        let run = Task { await cards.run("a", held.request) }
        await held.waitForStart()
        #expect(cards["a"].busy)
        let second = Task { await cards.run("a", held.request) }
        for _ in 0..<20 { await Task.yield() }
        #expect(held.started == 1)
        held.answer(.failure(CoreError("This addon now needs setup first", code: "needsSetup")))
        #expect(await run.value == InstallCards.Card(busy: false, code: "needsSetup", words: "This addon now needs setup first"))
        #expect(await second.value == nil)
        #expect(cards["a"] == InstallCards.Card(busy: false, code: "needsSetup", words: "This addon now needs setup first"))
        #expect(cards["b"] == InstallCards.Card())
        // A success clears the card.
        let before = held.started
        let again = Task { await cards.run("a", held.request) }
        await held.waitForStart(before + 1)
        held.answer(.success(()))
        #expect(await again.value == InstallCards.Card())
        #expect(cards["a"] == InstallCards.Card())
    }

    @MainActor @Test func aResultFromBeforeANewProfileLandsNowhere() async {
        let cards = InstallCards()
        let held = Held()
        let run = Task { await cards.run("a", held.request) }
        await held.waitForStart()
        cards.forgetAll()
        #expect(cards["a"] == InstallCards.Card())
        held.answer(.failure(CoreError("This addon's server did not answer in time", code: "timeout")))
        #expect(await run.value == nil)
        #expect(cards["a"] == InstallCards.Card())
    }

    @MainActor @Test func aCancelledRequestLeavesNothing() async {
        let cards = InstallCards()
        let held = Held()
        let run = Task { await cards.run("a", held.request) }
        await held.waitForStart()
        held.answer(.failure(CancellationError()))
        #expect(await run.value == nil)
        #expect(cards["a"] == InstallCards.Card())
    }

    @MainActor @Test func tryAgainForgetsResultsButKeepsRunningCards() async {
        let cards = InstallCards()
        let held = Held()
        let failed = Task { await cards.run("done", held.request) }
        await held.waitForStart()
        held.answer(.failure(CoreError("This addon's server could not be reached", code: "unreachable")))
        _ = await failed.value
        let run = Task { await cards.run("a", held.request) }
        await held.waitForStart(2)
        cards.forgetAll(running: false)
        #expect(cards["done"] == InstallCards.Card())
        #expect(cards["a"].busy)
        held.answer(.failure(CoreError("This addon's server did not answer in time", code: "timeout")))
        _ = await run.value
        #expect(cards["a"].words == "This addon's server did not answer in time")
        #expect(cards["a"].code == "timeout")
    }

    @MainActor @Test func aNewInstalledStateForgetsOnlyAFinishedResult() async {
        let cards = InstallCards()
        let held = Held()
        let run = Task { await cards.run("a", held.request) }
        await held.waitForStart()
        cards.forget("a")
        #expect(cards["a"].busy)
        held.answer(.failure(CoreError("This addon is not the one the list named", code: "mismatch")))
        _ = await run.value
        cards.forget("a")
        #expect(cards["a"] == InstallCards.Card())
    }
}
