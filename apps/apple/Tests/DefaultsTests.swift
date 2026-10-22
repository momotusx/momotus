import Foundation
import Testing

// The recommended addons (plans/m13-default-addons.md 6 and 7): the rules
// on their own, then the step, the Home card, the check's fix and the link
// field through a real core opened in a temporary directory.

private func addon(_ key: String, _ installed: DefaultAddonInfo.Installed, group: String = "lists") -> DefaultAddonInfo {
    DefaultAddonInfo(key: key, name: key.capitalized, line: "", group: group, installed: installed, logo: nil)
}

private let mixed = [
    addon("cinemeta", .off, group: "catalogs"), addon("torrentio", .absent, group: "streams"),
    addon("subsense", .on, group: "subtitles"), addon("trakt", .absent),
]

private func on(_ choices: [DefaultChoice]) -> [String: Bool] {
    Dictionary(uniqueKeysWithValues: choices.map { ($0.key, $0.on) })
}

@Test func continueNamesExactlyTheRowsWithASwitch() {
    // Only a missing default has a switch: one the profile has (on or off) is left out and keeps its state.
    #expect(on(DefaultsRules.choices(mixed, moved: [:])) == ["torrentio": true, "trakt": true])
    #expect(on(DefaultsRules.choices(mixed, moved: ["trakt": false, "cinemeta": true])) == ["torrentio": true, "trakt": false])
    // A new profile has none: all nine are named (an empty list would write nothing).
    let new = mixed.map { addon($0.key, .absent, group: $0.group) }
    #expect(on(DefaultsRules.choices(new, moved: ["subsense": false])) == ["cinemeta": true, "torrentio": true, "subsense": false, "trakt": true])
    // The card's Review shows the missing ones only.
    #expect(DefaultsRules.shown(mixed, scope: .missing).map(\.key) == ["torrentio", "trakt"])
    #expect(DefaultsRules.shown(mixed, scope: .all).count == 4)
}

@Test func aDefaultTheProfileHasSaysSoInsteadOfASwitch() {
    #expect(mixed.map(DefaultsRules.hasSwitch) == [false, true, false, true])
    #expect(DefaultsRules.presentWord(mixed[2], imported: true) == "From Stremio")
    #expect(DefaultsRules.presentWord(mixed[2], imported: false) == "Installed")
    #expect(DefaultsRules.presentWord(mixed[0], imported: false) == "Installed, off")
}

@Test func switchingEveryCatalogOffWarns() {
    let list = [addon("cinemeta", .absent, group: "catalogs"), addon("kitsu", .absent, group: "catalogs"), addon("torrentio", .absent, group: "streams")]
    #expect(!DefaultsRules.warns(list, moved: ["cinemeta": false], hasCatalog: false))
    #expect(DefaultsRules.warns(list, moved: ["cinemeta": false, "kitsu": false], hasCatalog: false))
    // A catalog already on (an import brought one) needs no warning.
    #expect(!DefaultsRules.warns(list, moved: ["cinemeta": false, "kitsu": false], hasCatalog: true))
    #expect(!DefaultsRules.warns([addon("cinemeta", .on, group: "catalogs")], moved: [:], hasCatalog: false))
    #expect(DefaultsRules.warns([addon("cinemeta", .off, group: "catalogs")], moved: [:], hasCatalog: false))
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
    #expect(DefaultsRules.namesLine(["Torrentio", "Trakt", "OpenSubtitles v3"]) == "Torrentio, Trakt and OpenSubtitles v3")
    #expect(DefaultsRules.namesLine(["Trakt"]) == "Trakt")
}

@Test func theChecksFixIsItsRecommendedAddonWhenMissingOrOff() {
    let list = DefaultAddons(version: 1, addons: mixed)
    #expect(DefaultsRules.fix("cinemeta", in: list)?.installed == .off)
    #expect(DefaultsRules.fix("torrentio", in: list)?.installed == .absent)
    #expect(DefaultsRules.fix("subsense", in: list) == nil)
    #expect(DefaultsRules.fix("cinemeta", in: nil) == nil)
    #expect(DefaultsRules.fixWords(mixed[0]) == "Turn Cinemeta on")
    #expect(DefaultsRules.fixWords(mixed[1]) == "Add Torrentio")
}

@Test func aSwitchIsWhiteWhenOnAndEmptyWhenOffNeverTheAccent() {
    #expect(SwitchLook.of(on: true) == SwitchLook(track: .primary, edge: nil, knob: .onPrimary))
    #expect(SwitchLook.of(on: false) == SwitchLook(track: nil, edge: .control, knob: .muted))
    // TV and the Mac say On or Off beside it; a phone does not.
    #expect(SwitchLook.showsWord(wide: true))
    #expect(!SwitchLook.showsWord(wide: false))
}

@Test func theStepsAreOneCenteredColumnOnWideScreens() {
    #expect(DefaultsRules.column(wide: false, tv: false) == nil)
    #expect(DefaultsRules.column(wide: true, tv: false) == 880)
    #expect(DefaultsRules.column(wide: true, tv: true) == 1400)
}

@Test func onlyADebugWalkOpensTheStepDirectly() {
    #expect(DefaultsRules.walkOpensDefaults("defaults", debug: true))
    // A Release build never reads it, and nothing else opens the step.
    #expect(!DefaultsRules.walkOpensDefaults("defaults", debug: false))
    #expect(!DefaultsRules.walkOpensDefaults(nil, debug: true))
    #expect(!DefaultsRules.walkOpensDefaults("check", debug: true))
}

@Test func newFieldsDecode() throws {
    let snapshot = try JSONDecoder().decode(AddonsSnapshot.self, from: Data(#"{"addons": [], "fresh": true}"#.utf8))
    #expect(snapshot.fresh)
    // Cinemeta has no logo (null), Torrentio one on the loopback server, and a core before logos sends no field.
    let list = try JSONDecoder().decode(DefaultAddons.self, from: Data(#"""
        {"version": 1, "addons": [
         {"key": "cinemeta", "name": "Cinemeta", "line": "Titles.", "group": "catalogs", "installed": "off", "logo": null},
         {"key": "torrentio", "name": "Torrentio", "line": "Streams.", "group": "streams", "installed": "absent",
          "logo": "http://127.0.0.1:7781/logo/0123abcd/torrentio"},
         {"key": "trakt", "name": "Trakt", "line": "Lists.", "group": "lists", "installed": "on"}]}
        """#.utf8))
    #expect(list.addons.map(\.logo) == [nil, "http://127.0.0.1:7781/logo/0123abcd/torrentio", nil])
    #expect(list.addons[0] == DefaultAddonInfo(key: "cinemeta", name: "Cinemeta", line: "Titles.", group: "catalogs", installed: .off, logo: nil))
}

@Test func aRowWithoutALogoShowsItsInitial() {
    #expect(DefaultsRules.initial("Cinemeta") == "C")
    #expect(DefaultsRules.initial("anime Kitsu") == "A")
    // Leading marks are skipped: the first letter or digit.
    #expect(DefaultsRules.initial("[NEW] TurkceAltyazi") == "N")
    #expect(DefaultsRules.initial("123 Movies") == "1")
    #expect(DefaultsRules.initial("") == "")
}

@Suite(.serialized) @MainActor struct DefaultsWiringTests {
    private func installed(_ core: CoreClient) async throws -> [String: DefaultAddonInfo.Installed] {
        let list = try await core.call("defaultAddons", as: DefaultAddons.self)
        return Dictionary(uniqueKeysWithValues: list.addons.map { ($0.key, $0.installed) })
    }

    /// A profile from an older device: the ten installed, never offered (the core counts installDefaults as seen).
    private func older(_ core: CoreClient) async throws {
        let list = try await core.call("defaultAddons", as: DefaultAddons.self)
        try await core.perform("installDefaults", ["choices": list.addons.map { DefaultChoice(key: $0.key, on: true).fields }])
        try await core.perform("setPrefs", ["patch": ["defaultsSeen": 0]])
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
        #expect(step.rows.count == 10)
        #expect(step.rows.allSatisfy { DefaultsRules.hasSwitch($0) && step.isOn($0) })
        for row in step.rows where ["torrentio", "trakt"].contains(row.key) { step.flip(row) }
        #expect(await step.install())
        let state = try await installed(core.client)
        #expect(state.count == 10)
        #expect(state.filter { $0.value == .off }.keys.sorted() == ["torrentio", "trakt"])
        #expect(state.filter { $0.value == .on }.count == 8)
        #expect(await eventually { !offer.fresh })
        // The profile has seen this list: a default removed later brings no card.
        #expect(await eventually { offer.prefs.snapshot?.defaultsSeen == 1 })
        try await core.client.perform("removeAddon", ["url": try #require(await url("SubSense", offer))])
        await offer.refresh()
        #expect(offer.list?.addons.first { $0.key == "subsense" }?.installed == .absent)
        #expect(offer.names(kids: false) == nil)
    }

    @Test func continueBeforeTheListIsReadStillNamesAllTen() async throws {
        let core = RealCore()
        #expect(await core.ready())
        // Continue pressed before the list arrived.
        #expect(await DefaultsStep(core: core.client, scope: .all).install())
        #expect(try await installed(core.client).values.allSatisfy { $0 == .on })
    }

    @Test func theCardsReviewAddsTheMissingAndKeepsAnOffOneOff() async throws {
        let core = RealCore()
        #expect(await core.ready())
        let offer = DefaultsOffer(core: core.client)
        offer.start()
        try await older(core.client)
        let torrentio = try #require(await url("Torrentio", offer))
        let cinemeta = try #require(await url("Cinemeta", offer))
        try await core.client.perform("removeAddon", ["url": torrentio])
        try await core.client.perform("setAddonEnabled", ["url": cinemeta, "enabled": false])
        #expect(await eventually { offer.addons.snapshot?.addons.count == 9 && offer.prefs.snapshot != nil })
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
        try await older(core.client)
        try await core.client.perform("removeAddon", ["url": try #require(await url("Trakt Tv", offer))])
        #expect(await eventually { offer.addons.snapshot?.addons.count == 9 && offer.prefs.snapshot != nil })
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
        try await older(core.client)
        for name in ["Cinemeta", "OpenSubtitles v3"] {
            try await core.client.perform("setAddonEnabled", ["url": try #require(await url(name, offer)), "enabled": false])
        }
        try await core.client.perform("removeAddon", ["url": try #require(await url("Torrentio", offer))])
        let fixer = DefaultsStep(core: core.client, scope: .all)
        await fixer.load()
        #expect(DefaultsRules.fix("cinemeta", in: fixer.list)?.installed == .off)
        #expect(await fixer.set("cinemeta", on: true))
        var state = try await installed(core.client)
        #expect(state["cinemeta"] == .on)
        #expect(state["opensubtitles"] == .off)
        #expect(state["torrentio"] == .absent)
        // A missing one is added by itself.
        #expect(await fixer.set("torrentio", on: true))
        state = try await installed(core.client)
        #expect(state["torrentio"] == .on)
        #expect(state["opensubtitles"] == .off)
        #expect(state.filter { $0.value == .on }.count == 9)
        // Settings switches one where it is, and reads the list again.
        #expect(await fixer.set("trakt", on: false))
        #expect(fixer.list?.addons.first { $0.key == "trakt" }?.installed == .off)
        #expect(try await installed(core.client).filter { $0.value == .on }.count == 8)
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
        // Editing the link drops the warning; the same link asks again.
        install.link = link + " "
        #expect(install.refused == nil)
        install.link = link
        await install.install()
        #expect(install.refused == link)
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
