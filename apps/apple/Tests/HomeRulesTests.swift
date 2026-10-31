import Foundation
import Testing

// Home's rules, as Android's RouteTest has them.

private func row(_ state: RowState, _ tiles: String...) -> HomeRow {
    HomeRow(
        rowId: "r\(tiles.count)", title: "Popular", source: "Cinemeta", state: state,
        tiles: tiles.map {
            Tile(
                id: $0, kind: "movie", name: $0, poster: nil, posterFallback: nil, background: nil, logo: nil, posterShape: "poster",
                releaseInfo: nil, progress: nil, videoId: nil, rating: nil, ratings: nil)
        })
}

@Test func heroPlayIsTheCoresTargetAndAPageWithoutOneHasNone() throws {
    func hero(_ play: String) throws -> Hero {
        try JSONDecoder().decode(Hero.self, from: Data(#"{"tile":{"id":"tt9","kind":"series","name":"Carrie","posterShape":"poster"},"reason":"Popular Series"\#(play)}"#.utf8))
    }
    // A series plays the episode the core picked, in the title page's words (the owner: Carrie had Details alone).
    let series = try hero(#","play":{"videoId":"tt9:2:3","label":"Resume S2:E3","resumeMs":1200,"leftMs":null}"#)
    #expect(series.play == PlayTarget(videoId: "tt9:2:3", label: "Resume S2:E3", resumeMs: 1200, leftMs: nil))
    // No meta answer yet, or a reply from before the field: Details alone.
    #expect(try hero(#","play":null"#).play == nil)
    #expect(try hero("").play == nil)
    // Play's place is held only while the core says it may still come.
    #expect(try hero(#","play":null,"playPending":true"#).holdsPlaySlot)
    #expect(try !hero(#","play":null,"playPending":false"#).holdsPlaySlot)
    #expect(!series.holdsPlaySlot)
    #expect(try !hero("").holdsPlaySlot)
}

@Test func tilesKeepTheirIdentityWhenTheyMove() {
    let a = row(.ready, "a").tiles[0], b = row(.ready, "b").tiles[0]
    #expect([a, b].keyed.map(\.key) == ["movie:a", "movie:b"])
    // After a play Continue Watching reorders: the keys move with their tiles.
    #expect([b, a].keyed.map(\.key) == ["movie:b", "movie:a"])
    // A tile twice in one strip still has two keys.
    #expect([a, a].keyed.map(\.key) == ["movie:a", "movie:a#1"])
}

@Test func tvHomeEntersOnContinueWatchingThenTheHeroThenTheFirstRowWithTiles() {
    let watching = HomeRow(rowId: "continue-watching", title: "Continue Watching", source: "", state: .ready, tiles: row(.ready, "a").tiles)
    let popular = HomeRow(rowId: "popular", title: "Popular", source: "Cinemeta", state: .ready, tiles: row(.ready, "b").tiles)
    let failed = HomeRow(rowId: "broken", title: "Broken", source: "X", state: .failed(reason: "500"), tiles: [])
    #expect(HomeStart.target(rows: [watching, popular], hero: true) == .row("continue-watching"))
    #expect(HomeStart.target(rows: [popular], hero: true) == .hero)
    // No hero: the first row with tiles, past an empty or failed lane.
    #expect(HomeStart.target(rows: [failed, popular], hero: false) == .row("popular"))
    // Continue Watching that is not the first row does not lead.
    #expect(HomeStart.target(rows: [popular, watching], hero: true) == .hero)
    #expect(HomeStart.target(rows: [failed], hero: false) == nil)
}

@Test func homeOffersOneTryAgainOnlyWhenEveryLaneEndedWithNothingAndOneFailed() {
    let failed = row(.failed(reason: "offline"))
    #expect([failed, failed].allFailed)
    #expect([failed, row(.empty)].allFailed)
    #expect(![failed, row(.loading)].allFailed, "a lane still on its way")
    #expect(![failed, row(.slow)].allFailed, "a slow lane is still on its way")
    #expect(![failed, row(.ready, "a")].allFailed, "a lane with tiles")
    #expect(![row(.failed(reason: "late"), "a")].allFailed, "tiles that arrived before the failure")
    #expect(![row(.empty)].allFailed, "empty, not failed")
    #expect(![HomeRow]().allFailed)
}

@Test func aRowNeverNamesItsAddonNotEvenForATwinOrInItsState() {
    // The owner's rule (2026-10-22): two "Popular Movies" rows, or Search's two "Movies" groups,
    // still show the title alone; a slow, empty or failed row says what happened.
    func lane(_ id: String, _ state: RowState) -> HomeRow {
        HomeRow(rowId: id, title: "Popular Movies", source: "Cinemeta", state: state, tiles: [])
    }
    #expect(lane("cinemeta/movie/top", .ready).subtitle == nil)
    #expect(lane("other/movie/top", .ready).subtitle == nil)
    #expect(lane("cinemeta/movie/top", .slow).subtitle == "Slow to answer, still trying")
    #expect(lane("cinemeta/movie/top", .ready).stateWords == nil)
    #expect(lane("cinemeta/movie/top", .empty).stateWords == "Nothing in this row right now.")
    #expect(lane("cinemeta/movie/top", .failed(reason: "timed out")).stateWords == "This row did not load.")
}

@Test func homeSearchDiscoverAndTheTitleNeverReadAnAddonsName() throws {
    // The rows, the hero, Search's groups, Discover's catalog chips and the title page never read
    // `source` or a version's addon (`via`); only the Addons pages,
    // Settings > Home rows and the Versions page name an addon (design/DECISION.md, Shelves).
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    var read = 0
    for folder in ["Home", "Search", "Discover", "Title"] {
        for name in try FileManager.default.contentsOfDirectory(atPath: app.appending(path: folder).path) where name.hasSuffix(".swift") {
            let text = try String(contentsOf: app.appending(path: "\(folder)/\(name)"), encoding: .utf8)
            #expect(text.range(of: #"\.source\b|\.via\b"#, options: .regularExpression) == nil, "\(folder)/\(name) reads an addon's name")
            read += 1
        }
    }
    #expect(read >= 13)
}

@Test func settingsAddonsRowCountsAndNamesNoAddon() {
    func addon(_ name: String, streams: Bool, enabled: Bool = true) -> AddonInfo {
        AddonInfo(url: "https://\(name).example/manifest.json", host: "\(name).example", name: name, description: nil, logo: nil,
                  version: "1.0.0", enabled: enabled, provides: streams ? ["stream"] : ["catalog"], types: ["movie"],
                  configurable: false, configureUrl: nil, builtIn: nil)
    }
    let cinemeta = addon("Cinemeta", streams: false), torrentio = addon("Torrentio", streams: true)
    #expect(AddonsSnapshot(addons: [cinemeta], fresh: false).summary == "1 installed. None of them finds streams yet.")
    #expect(AddonsSnapshot(addons: [cinemeta, torrentio], fresh: false).summary == "2 installed. 1 finds streams.")
    let more = [cinemeta, torrentio, addon("Comet", streams: true), addon("Off", streams: true, enabled: false)]
    #expect(AddonsSnapshot(addons: more, fresh: false).summary == "4 installed. 2 find streams.")
    #expect(AddonsSnapshot(addons: [], fresh: true).summary == "None installed yet.")
}

@Test func oneFailedRowsTryAgainAsksForThatRowOnly() {
    let row = HomeRow(rowId: "cinemeta/movie/top", title: "Popular Movies", source: "Cinemeta", state: .failed(reason: "timed out"), tiles: [])
    // The core's retryRow on the open Home view, with the row's id as the snapshot gave it.
    #expect(row.retryCommand == ["type": "retryRow", "rowId": "cinemeta/movie/top"])
}

private func card(_ id: String) -> Tile {
    Tile(
        id: id, kind: "series", name: "Title \(id)", poster: nil, posterFallback: nil, background: nil, logo: nil, posterShape: "landscape",
        releaseInfo: nil, progress: 0.4, videoId: nil, rating: nil, ratings: nil)
}

private struct Refused: LocalizedError { var errorDescription: String? { "timed out" } }

@MainActor private final class Sent { var ids: [String] = []; var fail = false }

@MainActor @Test func aRemovalWaitsForItsUndoThenGoesToTheCore() async throws {
    let sent = Sent()
    let removal = ContinueRemoval(wait: .milliseconds(200)) { tile in
        if sent.fail { throw Refused() }
        sent.ids.append(tile.id)
    }
    // Out of the row at once, nothing sent until the window ends; then sent once, and Undo is gone.
    removal.remove(card("a"))
    #expect(removal.removing == card("a") && removal.hides(card("a")) && sent.ids.isEmpty)
    try await Task.sleep(for: .milliseconds(800))
    #expect(sent.ids == ["a"] && removal.removing == nil && !removal.hides(card("a")))
    // Undo: back, and nothing is sent.
    removal.remove(card("b"))
    removal.undo()
    #expect(!removal.hides(card("b")))
    try await Task.sleep(for: .milliseconds(800))
    #expect(sent.ids == ["a"])
    // A second removal sends the first at once, never twice.
    removal.remove(card("c"))
    removal.remove(card("d"))
    try await Task.sleep(for: .milliseconds(50))
    #expect(sent.ids == ["a", "c"] && removal.removing == card("d") && removal.hides(card("d")))
    try await Task.sleep(for: .milliseconds(800))
    #expect(sent.ids == ["a", "c", "d"])
    // Refused: the title is back, and the line says why.
    sent.fail = true
    removal.remove(card("e"))
    try await Task.sleep(for: .milliseconds(800))
    #expect(removal.removing == nil && !removal.hides(card("e")) && removal.failed?.tile == card("e"))
    #expect(removal.failed?.words == "Title e could not be removed from Continue Watching: timed out")
    #expect(removedWords(name: "Title e") == "Title e was removed from Continue Watching.")
}

@MainActor @Test func aTitleOnItsWayToTheCoreHasNoUndo() async throws {
    // The window ended and the send is under way: Undo is gone, so it can never bring back a title the core removes.
    let sent = Sent()
    let gate = AsyncStream<Void>.makeStream()
    let removal = ContinueRemoval(wait: .milliseconds(300)) { tile in
        for await _ in gate.stream { break }
        sent.ids.append(tile.id)
    }
    removal.remove(card("a"))
    try await Task.sleep(for: .milliseconds(600))
    #expect(removal.removing == nil && removal.sending == card("a") && removal.hides(card("a")))
    // Another removal now does not send the first again; the core's answer lets the first back out of hiding.
    removal.remove(card("b"))
    gate.continuation.yield()
    try await Task.sleep(for: .milliseconds(60))
    #expect(sent.ids == ["a"] && removal.sending == nil)
    #expect(removal.removing == card("b"))
    removal.undo()
}

@Test func continueWatchingIsReachableWithoutALongPressAndUndoes() throws {
    let here = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
    let rows = try String(contentsOf: here.appending(path: "../Marquee/Home/RowView.swift"), encoding: .utf8)
    // The phone's X on each card, the row leaving out a title that waits, and the Undo line under it.
    let strip = try body(of: "private var strip: some View", in: rows)
    #expect(strip.contains("let tiles = (continues ? row.tiles.filter { removal?.hides($0) != true } : row.tiles).keyed"))
    #expect(strip.contains("#if os(iOS)") && strip.contains("RemoveTileButton(name: tile.name) { removal.remove(tile) }"), "the card lost its X")
    let line = try body(of: "private struct RemovalLine: View", in: rows)
    #expect(line.contains("action: (\"Undo\", removal.undo)"))
    #expect(try String(contentsOf: here.appending(path: "../Marquee/Home/HomeScreen.swift"), encoding: .utf8).contains("removal: model.removal)"))
    // Start over in sight beside Resume (Mac, iPad, TV), and on the iPhone with Trailer and Versions.
    let actions = try String(contentsOf: here.appending(path: "../Marquee/Title/TitleActions.swift"), encoding: .utf8)
    let play = try body(of: "struct PlayBlock: View", in: actions)
    #expect(play.contains("if let resume = target?.resumeMs, resume > 0, wide || !v3 {\n                        Button { play(true) } label: {"))
    #expect(!play.contains(".contextMenu"), "Start over went back into a long press")
    let title = try String(contentsOf: here.appending(path: "../Marquee/Title/TitleScreen.swift"), encoding: .utf8)
    let phone = try body(of: "private func phoneSecondaries(_ details: TitleDetails) -> some View", in: title)
    #expect(phone.contains("Button { play(fromStart: true) } label: { Label(\"Start over\", glyph: \"rotate-ccw\") }"))
}

@MainActor @Test func playingATitleWithinItsUndoWindowKeepsItInContinueWatching() async throws {
    let sent = Sent()
    let removal = ContinueRemoval(wait: .milliseconds(200)) { sent.ids.append($0.id) }
    removal.remove(card("a"))
    // Another title playing changes nothing; this one playing drops the removal, and nothing is sent.
    ContinueRemoval.played(itemId: "b")
    #expect(removal.removing == card("a"))
    ContinueRemoval.played(itemId: "a")
    #expect(removal.removing == nil && !removal.hides(card("a")))
    try await Task.sleep(for: .milliseconds(600))
    #expect(sent.ids.isEmpty, "a played title was removed when the time ended")
    // Every play starts through PlayerModel's init, which says so first.
    let model = try String(contentsOf: URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Player/PlayerModel.swift"), encoding: .utf8)
    #expect(try body(of: "init(route: PlayRoute, versions: VersionsModel?, core: CoreClient)", in: model).contains("ContinueRemoval.played(itemId: route.target.itemId)"))
}
