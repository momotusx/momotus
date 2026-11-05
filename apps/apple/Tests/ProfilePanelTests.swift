import Foundation
import Testing

// The Mac's profile panel as values (plans/profile-panel.md, the owner's pick 7): its words, its card and its rows.

private func profile(_ id: String, pin: Bool = false, owner: Bool = false, syncing: Bool = false, kids: Bool = false) -> ProfileInfo {
    ProfileInfo(id: id, name: id.capitalized, avatar: "", position: 0, hasPin: pin, isOwner: owner, syncing: syncing, kids: kids)
}

private func snapshot(_ profiles: [ProfileInfo], active: String? = nil, locked: Bool = false, host: String? = nil,
                      pin: String? = nil) -> ProfilesSnapshot {
    ProfilesSnapshot(profiles: profiles, active: active ?? profiles[0].id, locked: locked, max: 6,
                     incognitoHost: host, incognitoPin: pin)
}

private func watched(_ id: String, _ label: String, spoken: String? = nil, art: String? = "https://art/\(UUID())") -> LastWatched {
    LastWatched(profile: id, itemId: "tt1", label: label, spoken: spoken ?? label, art: art)
}

private let main = profile("main", owner: true)
private let robin = profile("robin", pin: true)
private let ada = profile("ada")
private let kid = profile("kid", kids: true)

@Test func theAnswerDecodesTheGeneratedShape() throws {
    let json = #"[{"profile":"ada","itemId":"tt5875444","label":"Slow Horses, S4:E2","spoken":"Slow Horses, season 4, episode 2","art":null}]"#
    let list = try JSONDecoder().decode([LastWatched].self, from: Data(json.utf8))
    #expect(list == [LastWatched(profile: "ada", itemId: "tt5875444", label: "Slow Horses, S4:E2", spoken: "Slow Horses, season 4, episode 2", art: nil)])
}

@Test func theWordsAreTheWebPanels() {
    #expect(ProfilePanelModel.title == "Profiles")
    #expect(ProfilePanelModel.watchingNow == "Watching now")
    #expect(ProfilePanelModel.lastWatched == "Last watched")
    #expect(ProfilePanelModel.switchTo == "Switch to")
    #expect(ProfilePanelModel.incognitoLine == "Watch without keeping anything")
    #expect(ProfilePanelModel.manage == "Manage profiles")
    #expect(ProfilePanelModel.settings == "Settings")
}

@Test func theCardIsTheCurrentProfileOnItsLastTitle() throws {
    let silo = watched("main", "Silo, S2:E4", spoken: "Silo, season 2, episode 4")
    let card = try #require(ProfilePanelModel.card(snapshot([main, ada]), last: ["main": silo]))
    #expect(card.profile == main && card.sub == "Watching now" && card.last == silo && !card.leave)
    // Nothing watched: the card stays plain, no line.
    #expect(ProfilePanelModel.card(snapshot([main, ada]), last: [:])?.last == nil)
}

@Test func rowsAreTheOtherProfilesInOrderWithTheirLabelsThenIncognito() {
    let last = ["main": watched("main", "Silo, S2:E4"), "ada": watched("ada", "Dune", spoken: "Dune")]
    let rows = ProfilePanelModel.rows(snapshot([main, ada, robin]), last: last)
    #expect(rows.map(\.id) == ["ada", "robin", Incognito.id])
    #expect(rows[0].line == "Dune" && rows[0].art == last["ada"]?.art && !rows[0].lock && !rows[0].disabled)
    #expect(rows[0].spoken == "Ada, last watched Dune")
    #expect(rows[2].name == "Incognito" && rows[2].line == "Watch without keeping anything" && rows[2].profile == nil)
    #expect(rows[2].spoken == "Incognito, Watch without keeping anything")
}

@Test func aPinProfileWithNoAnswerShowsItsLockAndNoTitle() {
    // The core leaves a PIN profile's entry out for other viewers: the row is the lock and the name.
    let rows = ProfilePanelModel.rows(snapshot([ada, robin]), last: ["ada": watched("ada", "Dune")])
    let row = rows[0]
    #expect(row.id == "robin" && row.lock && row.line == nil && row.art == nil)
    #expect(row.spoken == "Robin, locked with a PIN")
}

@Test func aKidsSessionShowsNoTitlesAndAsksForAGrownUp() throws {
    let s = snapshot([main, kid, ada], active: "kid")
    #expect(!ProfilePanelModel.asks(s))
    // Even an answer that reached the app shows nothing.
    let last = ["main": watched("main", "Silo, S2:E4"), "kid": watched("kid", "Bluey"), "ada": watched("ada", "Dune")]
    let rows = ProfilePanelModel.rows(s, last: last)
    #expect(rows.map(\.id) == ["main", "ada"], "Incognito is no way out of a kids session")
    #expect(rows.allSatisfy { $0.line == nil && $0.art == nil })
    #expect(rows[0].spoken == "Main, needs a grown-up's PIN")
    #expect(try #require(ProfilePanelModel.card(s, last: last)).last == nil)
    #expect(!ProfilePanelModel.manages(s))
}

@Test func incognitoShowsItsOwnCardWithLeaveAndNoTitlesOrRow() throws {
    let s = snapshot([main, ada], active: Incognito.id, host: "main")
    #expect(!ProfilePanelModel.asks(s))
    let last = ["main": watched("main", "Silo, S2:E4"), "ada": watched("ada", "Dune")]
    let card = try #require(ProfilePanelModel.card(s, last: last))
    #expect(card.profile == Incognito.profile && card.sub == Incognito.keptNothing && card.leave && card.last == nil)
    let rows = ProfilePanelModel.rows(s, last: last)
    // The host too is a row to switch to; exactly one Incognito, the card.
    #expect(rows.map(\.id) == ["main", "ada"])
    #expect(rows.allSatisfy { $0.line == nil && $0.art == nil })
    #expect(!ProfilePanelModel.manages(s))
}

@Test func theIncognitoRowIsLockedWhenAPinOpensIt() {
    let rows = ProfilePanelModel.rows(snapshot([main, ada], pin: "main"), last: [:])
    let row = rows.last
    #expect(row?.id == Incognito.id && row?.lock == true)
    #expect(row?.spoken == "Incognito, needs a PIN, Watch without keeping anything")
}

@Test func aSyncingProfileIsDisabledAndSaysSo() throws {
    let syncing = profile("bo", syncing: true)
    let rows = ProfilePanelModel.rows(snapshot([main, syncing]), last: ["bo": watched("bo", "Dune")])
    #expect(rows[0].disabled && rows[0].line == "Syncing" && rows[0].art == nil && !rows[0].lock)
    #expect(rows[0].spoken == "Bo, still syncing")
    // The current profile still syncing: the card says so and shows no title.
    let card = try #require(ProfilePanelModel.card(snapshot([syncing, main]), last: ["bo": watched("bo", "Dune")]))
    #expect(card.sub == "Syncing" && card.last == nil)
}

@Test func manageProfilesIsTheUnlockedOwners() {
    #expect(ProfilePanelModel.manages(snapshot([main, ada])))
    #expect(!ProfilePanelModel.manages(snapshot([ada, main])))
    #expect(!ProfilePanelModel.manages(snapshot([main, ada], locked: true)))
}
