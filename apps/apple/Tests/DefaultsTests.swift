import Foundation
import Testing

// The recommended addons (plans/m13-default-addons.md 6 and 7): the rules
// on their own, then the step, the Home card, the check's fix and the link
// field through a real core opened in a temporary directory.

private func addon(_ key: String, _ installed: DefaultAddonInfo.Installed, group: String = "lists") -> DefaultAddonInfo {
    DefaultAddonInfo(key: key, name: key.capitalized, line: "", group: group, installed: installed)
}

private let mixed = [
    addon("cinemeta", .off, group: "catalogs"), addon("torrentio", .absent, group: "streams"),
    addon("subsense", .on, group: "subtitles"), addon("trakt", .absent),
]

private func on(_ choices: [DefaultChoice]) -> [String: Bool] {
    Dictionary(uniqueKeysWithValues: choices.map { ($0.key, $0.on) })
}

@Test func everyDefaultIsSentAndOneInTheListKeepsItsState() {
    // Missing ones only (the card): the off one is not shown and stays off; a key left out would mean on.
    let missing = DefaultsRules.choices(mixed, scope: .missing, moved: [:])
    #expect(missing.count == mixed.count)
    #expect(on(missing) == ["cinemeta": false, "torrentio": true, "subsense": true, "trakt": true])
    // Every one (first run, the check): the off one has a switch that starts off.
    #expect(on(DefaultsRules.choices(mixed, scope: .all, moved: [:]))["cinemeta"] == false)
    #expect(on(DefaultsRules.choices(mixed, scope: .all, moved: ["cinemeta": true, "trakt": false])) == [
        "cinemeta": true, "torrentio": true, "subsense": true, "trakt": false,
    ])
    // A switch moved on a row the step does not show changes nothing.
    #expect(on(DefaultsRules.choices(mixed, scope: .missing, moved: ["cinemeta": true]))["cinemeta"] == false)
}

@Test func onlyMissingOrOffDefaultsHaveASwitch() {
    #expect(DefaultsRules.shown(mixed, scope: .missing).map(\.key) == ["torrentio", "trakt"])
    #expect(DefaultsRules.shown(mixed, scope: .all).count == 4)
    #expect(mixed.map(DefaultsRules.hasSwitch) == [true, true, false, true])
    #expect(mixed.map(DefaultsRules.starts) == [false, true, true, true])
    #expect(DefaultsRules.presentWord(imported: true) == "From Stremio")
    #expect(DefaultsRules.presentWord(imported: false) == "Added")
}

@Test func turningTheCatalogOffWarns() {
    #expect(DefaultsRules.warns(DefaultsRules.choices(mixed, scope: .all, moved: [:])))
    #expect(!DefaultsRules.warns(DefaultsRules.choices(mixed, scope: .all, moved: ["cinemeta": true])))
}

@Test func theCardShowsOncePerVersionAndNeverOnKidsOrAtFirstRun() {
    let list = DefaultAddons(version: 1, addons: mixed)
    #expect(DefaultsRules.cardNames(list, seen: 0, kids: false, fresh: false) == ["Torrentio", "Trakt"])
    #expect(DefaultsRules.cardNames(list, seen: 1, kids: false, fresh: false) == nil)
    #expect(DefaultsRules.cardNames(list, seen: 0, kids: true, fresh: false) == nil)
    #expect(DefaultsRules.cardNames(list, seen: 0, kids: false, fresh: true) == nil)
    // Prefs or the list not read yet, or nothing missing.
    #expect(DefaultsRules.cardNames(list, seen: nil, kids: false, fresh: false) == nil)
    #expect(DefaultsRules.cardNames(nil, seen: 0, kids: false, fresh: false) == nil)
    #expect(DefaultsRules.cardNames(DefaultAddons(version: 1, addons: [mixed[0], mixed[2]]), seen: 0, kids: false, fresh: false) == nil)
}

@Test func theChecksFixReviewsWhenOneIsMissingElseTurnsTheGroupsOn() {
    // Installing appends every missing default: the user sees their switches first.
    #expect(DefaultsRules.fix(group: "catalogs", in: DefaultAddons(version: 1, addons: mixed)) == .review)
    let present = [addon("cinemeta", .off, group: "catalogs"), addon("subsense", .on, group: "subtitles"), addon("opensubtitles", .off, group: "subtitles")]
    #expect(DefaultsRules.fix(group: "catalogs", in: DefaultAddons(version: 1, addons: present)) == .turnOn(key: "cinemeta", name: "Cinemeta"))
    #expect(DefaultsRules.fix(group: "subtitles", in: DefaultAddons(version: 1, addons: present)) == .turnOn(key: "opensubtitles", name: "Opensubtitles"))
    #expect(DefaultsRules.fix(group: "streams", in: DefaultAddons(version: 1, addons: present)) == nil)
    #expect(DefaultsRules.fix(group: "catalogs", in: nil) == nil)
}

@Test func newFieldsDecode() throws {
    let snapshot = try JSONDecoder().decode(AddonsSnapshot.self, from: Data(#"{"addons": [], "fresh": true}"#.utf8))
    #expect(snapshot.fresh)
    let list = try JSONDecoder().decode(DefaultAddons.self, from: Data(#"""
        {"version": 1, "addons": [{"key": "cinemeta", "name": "Cinemeta", "line": "Titles.", "group": "catalogs", "installed": "off"}]}
        """#.utf8))
    #expect(list.addons == [DefaultAddonInfo(key: "cinemeta", name: "Cinemeta", line: "Titles.", group: "catalogs", installed: .off)])
}

@Suite(.serialized) @MainActor struct DefaultsWiringTests {
    private func installed(_ core: CoreClient) async throws -> [String: DefaultAddonInfo.Installed] {
        let list = try await core.call("defaultAddons", as: DefaultAddons.self)
        return Dictionary(uniqueKeysWithValues: list.addons.map { ($0.key, $0.installed) })
    }

    /// The installed addon a default became, by its name in the addons view.
    private func url(_ name: String, _ offer: DefaultsOffer) async -> String? {
        _ = await eventually { offer.addons.snapshot?.addons.contains { $0.name == name } == true }
        return offer.addons.snapshot?.addons.first { $0.name == name }?.url
    }

    @Test func firstRunInstallsTheSwitchesAndIsFreshNoMore() async throws {
        let core = RealCore()
        #expect(await core.ready())
        let offer = DefaultsOffer(core: core.client)
        offer.start()
        #expect(await eventually { offer.fresh })
        #expect(offer.names(kids: false) == nil)
        let step = DefaultsStep(core: core.client, scope: .all)
        await step.load()
        #expect(step.rows.count == 9)
        #expect(step.rows.allSatisfy { step.isOn($0) })
        for row in step.rows where ["torrentio", "trakt"].contains(row.key) { step.flip(row) }
        #expect(await step.install())
        let state = try await installed(core.client)
        #expect(state.count == 9)
        #expect(state.filter { $0.value == .off }.keys.sorted() == ["torrentio", "trakt"])
        #expect(state.filter { $0.value == .on }.count == 7)
        #expect(await eventually { !offer.fresh })
        // The profile has seen this list: a default removed later brings no card.
        #expect(await eventually { offer.prefs.snapshot?.defaultsSeen == 1 })
        try await core.client.perform("removeAddon", ["url": try #require(await url("SubSense", offer))])
        await offer.refresh()
        #expect(offer.list?.addons.first { $0.key == "subsense" }?.installed == .absent)
        #expect(offer.names(kids: false) == nil)
    }

    @Test func browseFirstInstallsAllOnWithoutReadingTheListFirst() async throws {
        let core = RealCore()
        #expect(await core.ready())
        // Browse first sends Continue on a step that never loaded.
        #expect(await DefaultsStep(core: core.client, scope: .all).install())
        #expect(try await installed(core.client).values.allSatisfy { $0 == .on })
    }

    @Test func theCardsReviewAddsTheMissingAndKeepsAnOffOneOff() async throws {
        let core = RealCore()
        #expect(await core.ready())
        let offer = DefaultsOffer(core: core.client)
        offer.start()
        try await core.client.perform("installDefaults", ["choices": [[String: any Sendable]]()])
        let torrentio = try #require(await url("Torrentio", offer))
        let cinemeta = try #require(await url("Cinemeta", offer))
        try await core.client.perform("removeAddon", ["url": torrentio])
        try await core.client.perform("setAddonEnabled", ["url": cinemeta, "enabled": false])
        #expect(await eventually { offer.addons.snapshot?.addons.count == 8 && offer.prefs.snapshot != nil })
        await offer.refresh()
        #expect(offer.names(kids: false) == ["Torrentio"])
        #expect(offer.names(kids: true) == nil)

        let review = DefaultsStep(core: core.client, scope: .missing)
        await review.load()
        #expect(review.rows.map(\.key) == ["torrentio"])
        #expect(await review.install())
        let state = try await installed(core.client)
        #expect(state["torrentio"] == .on)
        #expect(state["cinemeta"] == .off)
        #expect(await eventually { offer.prefs.snapshot?.defaultsSeen == 1 })
    }

    @Test func notNowAnswersTheCardOnce() async throws {
        let core = RealCore()
        #expect(await core.ready())
        let offer = DefaultsOffer(core: core.client)
        offer.start()
        try await core.client.perform("installDefaults", ["choices": [[String: any Sendable]]()])
        try await core.client.perform("removeAddon", ["url": try #require(await url("Trakt Tv", offer))])
        #expect(await eventually { offer.addons.snapshot?.addons.count == 8 && offer.prefs.snapshot != nil })
        await offer.refresh()
        #expect(offer.names(kids: false) == ["Trakt"])
        // Answered once: the card stays away, here and (synced) on every device.
        await offer.seen()
        #expect(await eventually { offer.prefs.snapshot?.defaultsSeen == 1 })
        #expect(offer.names(kids: false) == nil)
    }

    @Test func theChecksFixTurnsOneDefaultOnAndNothingElse() async throws {
        let core = RealCore()
        #expect(await core.ready())
        let offer = DefaultsOffer(core: core.client)
        offer.start()
        try await core.client.perform("installDefaults", ["choices": [[String: any Sendable]]()])
        for name in ["Cinemeta", "OpenSubtitles v3"] {
            try await core.client.perform("setAddonEnabled", ["url": try #require(await url(name, offer)), "enabled": false])
        }
        let fixer = DefaultsStep(core: core.client, scope: .all)
        await fixer.load()
        #expect(DefaultsRules.fix(group: "catalogs", in: fixer.list) == .turnOn(key: "cinemeta", name: "Cinemeta"))
        #expect(await fixer.turnOn("cinemeta"))
        let state = try await installed(core.client)
        #expect(state["cinemeta"] == .on)
        #expect(state["opensubtitles"] == .off)
        #expect(state.filter { $0.value == .on }.count == 8)
    }

    @Test func aLinkWithASignInWaitsForTheUsersWord() async throws {
        let core = RealCore()
        #expect(await core.ready())
        let install = LinkInstall(core: core.client)
        let link = "https://addon.invalid/%7B%22token%22%3A%22abc%22%7D/manifest.json"
        install.link = link
        await install.install()
        #expect(install.refused == link)
        #expect(install.problem == nil)
        #expect(install.link == link)
        // Its word given: sent again acknowledged, so the core goes on to fetch it (and this host does not exist).
        await install.install(acknowledged: true)
        #expect(install.refused == nil)
        let problem = try #require(install.problem)
        #expect(!problem.contains("sign-in"))
        // A plain link is never asked about.
        install.link = "https://addon.invalid/manifest.json"
        await install.install()
        #expect(install.refused == nil)
        #expect(install.problem != nil)
    }
}
