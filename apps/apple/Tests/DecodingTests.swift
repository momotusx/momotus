import Foundation
import Testing

// Fixtures are real bridge traffic recorded by scripts/capture-fixtures.ts:
// a fresh core, Cinemeta installed, Home opened, then Breaking Bad with
// selectSeason 2, setWatched on its first episode and setInLibrary, then
// Versions and Playback for the sample source's Big Buck Bunny.

private struct Reply<R: Decodable>: Decodable {
    let id: UInt64?
    let ok: Bool
    let result: R?
    let error: String?
}
private struct Opened<S: Decodable>: Decodable {
    let sub: UInt64
    let snapshot: S
}
private struct Push<U: Decodable>: Decodable {
    let sub: UInt64
    let updates: [U]
}

private func fixture<T: Decodable>(_ name: String, as type: T.Type) throws -> T {
    let url = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "Fixtures/\(name)")
    return try JSONDecoder().decode(T.self, from: Data(contentsOf: url))
}

@Test func homeSnapshotAndPushesReachTheCoreState() throws {
    let reply = try fixture("home-reply.json", as: Reply<Opened<HomeSnapshot>>.self)
    let opened = try #require(reply.result)
    #expect(opened.snapshot.rows.count == 6)
    #expect(opened.snapshot.rows.allSatisfy { $0.state == .loading && $0.tiles.isEmpty })

    var header = opened.snapshot.header
    var rows = opened.snapshot.rows
    for push in try fixture("home-pushes.json", as: [Push<HomeUpdate>].self) {
        #expect(push.sub == opened.sub)
        for update in push.updates {
            switch update {
            case .header(let value): header = value
            case .rows(let diffs): rows = applyDiffs(rows, diffs)
            }
        }
    }
    // Rows keep their positions while catalogs arrive.
    #expect(rows.map(\.rowId) == opened.snapshot.rows.map(\.rowId))
    #expect(rows.allSatisfy { $0.state == .ready && $0.tiles.count == 20 })
    #expect(rows.first?.title == "Popular Movies")
    let hero = try #require(header.hero)
    #expect(hero.reason == "#1 in Popular Movies on Cinemeta")
    #expect(hero.tile.background != nil)
}

@Test func emptyHomeHasNoRows() throws {
    let reply = try fixture("home-empty-reply.json", as: Reply<Opened<HomeSnapshot>>.self)
    #expect(try #require(reply.result).snapshot.rows.isEmpty)
}

@Test func titleFollowsSeasonWatchedAndLibraryCommands() throws {
    let reply = try fixture("title-reply.json", as: Reply<Opened<TitleSnapshot>>.self)
    let opened = try #require(reply.result)
    #expect(opened.snapshot.header.state == .loading)

    var header = opened.snapshot.header
    var episodes = opened.snapshot.episodes
    for push in try fixture("title-pushes.json", as: [Push<TitleUpdate>].self) {
        for update in push.updates {
            switch update {
            case .header(let value): header = value
            case .episodes(let diffs): episodes = applyDiffs(episodes, diffs)
            }
        }
    }
    #expect(header.state == .ready)
    #expect(header.details?.name == "Breaking Bad")
    #expect(header.seasons == [0, 1, 2, 3, 4, 5])
    #expect(header.selectedSeason == 2)
    #expect(header.inLibrary)
    #expect(header.playTarget?.label == "Play S1:E1")
    #expect(header.watched == false)
    #expect(header.details?.trailers.first?.youtubeId == "VFkjBy2b50Q")
    #expect(episodes.count == 13)
    #expect(episodes.allSatisfy { $0.season == 2 })
    #expect(episodes.first?.watched == true)
    #expect(episodes.dropFirst().allSatisfy { !$0.watched })
}

@Test func versionsArriveAsWholeSnapshotsWithThePick() throws {
    let reply = try fixture("versions-reply.json", as: Reply<Opened<VersionsSnapshot>>.self)
    let opened = try #require(reply.result)
    #expect(opened.snapshot.recommended == nil)
    #expect(opened.snapshot.pendingAddons == ["Sample Source"])
    // Record views push whole snapshots: the last one wins.
    let pushes = try fixture("versions-pushes.json", as: [Push<VersionsSnapshot>].self)
    let latest = try #require(pushes.last?.updates.last)
    #expect(latest.done)
    let pick = try #require(latest.recommended)
    #expect(pick.status == .direct)
    #expect(pick.reasons == ["playsDirectly"])
    #expect(pick.sizeBytes == 61_878_609)
    #expect(pick.via == "Sample Source")
}

@Test func playbackHandsOverALoopbackURL() throws {
    let reply = try fixture("playback-reply.json", as: Reply<Opened<PlaybackSnapshot>>.self)
    #expect(reply.ok)
    let pushes = try fixture("playback-pushes.json", as: [Push<PlaybackSnapshot>].self)
    let latest = try #require(pushes.last?.updates.last)
    #expect(latest.phase == "ready")
    #expect(URL(string: latest.url ?? "")?.host() == "127.0.0.1")
    #expect(latest.attempts.isEmpty)
    #expect(latest.subtitlesPending == false)
}

@Test func hiddenReasonsDecodeFromPairs() throws {
    let json = #"{"count": 3, "byReason": [["unplayable", 2], ["wrongTitle", 1]]}"#
    let hidden = try JSONDecoder().decode(HiddenSummary.self, from: Data(json.utf8))
    #expect(hidden.byReason.map(\.reason) == ["unplayable", "wrongTitle"])
    #expect(hidden.byReason.map(\.count) == [2, 1])
}

@Test func failedCommandsCarryTheCoreError() throws {
    let reply = try fixture("command-error-reply.json", as: Reply<[String: String]>.self)
    #expect(!reply.ok)
    #expect(reply.error == "unknown command: nope")
}
