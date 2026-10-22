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

/// Continue Watching comes first from the library, without artwork, and the
/// metas' artwork follows as a set on the same row (the core's 1728f97).
@Test func continueWatchingGainsArtworkInPlace() throws {
    let tile = #"{"id":"tt0903747","kind":"series","name":"Breaking Bad","poster":"https://p/bb.jpg","posterShape":"poster","releaseInfo":null,"progress":0.02,"videoId":"tt0903747:1:6","#
    let row = { (art: String) in #"{"rowId":"continue-watching","title":"Continue Watching","source":"","state":{"state":"ready"},"tiles":[\#(tile)\#(art)}]}"# }
    let pushes = """
        [{"sub":1,"updates":[{"kind":"rows","diffs":[{"op":"pushFront","value":\(row(#""background":null,"logo":null"#))}]}]},
         {"sub":1,"updates":[{"kind":"rows","diffs":[{"op":"set","index":0,"value":\(row(#""background":"https://b/s1e6.jpg","logo":"https://l/bb.png""#))}]}]}]
        """
    var rows = [HomeRow(rowId: "popular", title: "Popular Movies", source: "Cinemeta", state: .ready, tiles: [])]
    var seen: [Tile] = []
    for push in try JSONDecoder().decode([Push<HomeUpdate>].self, from: Data(pushes.utf8)) {
        for case .rows(let diffs) in push.updates { rows = applyDiffs(rows, diffs) }
        #expect(rows.map(\.rowId) == ["continue-watching", "popular"])
        seen.append(try #require(rows.first?.tiles.first))
    }
    #expect(seen.map(\.background) == [nil, "https://b/s1e6.jpg"])
    #expect(seen.map(\.logo) == [nil, "https://l/bb.png"])
    #expect(Set(seen.map(\.poster)) == ["https://p/bb.jpg"])
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
    // Recorded before "On this device" (plans/player-polish.md 2b): none.
    #expect(pick.onDevice == nil)
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

@Test func heroCarriesRatingSynopsisGenresAndRuntime() throws {
    // The shape mq-runtime's home test asserts (tests/home.rs).
    let json = #"""
    {"reason": "#1 in Popular Movies", "description": "A banker convicted of uxoricide forms a friendship.",
     "genres": ["Drama", "Crime", "Prison"], "runtime": "142 min",
     "tile": {"id": "tt0111161", "kind": "movie", "name": "The Shawshank Redemption", "poster": null, "background": null,
              "logo": null, "posterShape": "poster", "releaseInfo": "1994", "progress": null, "videoId": null, "rating": "9.3"}}
    """#
    let hero = try JSONDecoder().decode(Hero.self, from: Data(json.utf8))
    #expect(hero.tile.rating == "9.3")
    // Recorded before the scores existed: none to show, never a placeholder.
    #expect(hero.tile.ratings == nil)
    #expect(ratingParts(hero.tile.ratings).isEmpty)
    #expect(hero.genres == ["Drama", "Crime", "Prison"])
    #expect(hero.runtime == "142 min")
    #expect(hero.description?.hasPrefix("A banker") == true)
}

@Test func textShowsIMDbsNumberAloneAndAMissingScoreHasNoSlot() throws {
    // The shape mq-runtime's ratings test asserts (tests/ratings.rs).
    let json = #"""
    {"id": "tt0111161", "kind": "movie", "name": "The Shawshank Redemption", "poster": null, "background": null,
     "logo": null, "posterShape": "poster", "releaseInfo": "1994", "progress": null, "videoId": null, "rating": "9.3",
     "ratings": {"imdb": "9.2", "rottenTomatoes": "89%"}}
    """#
    let tile = try JSONDecoder().decode(Tile.self, from: Data(json.utf8))
    // Rotten Tomatoes shows only in the covers' band, never as text (the owner, 2026-10-23), as Stremio does.
    #expect(ratingParts(tile.ratings) == ["IMDb 9.2"])
    #expect(posterLabel([tile.name, tile.releaseInfo], tile.ratings) == "The Shawshank Redemption, 1994, IMDb 9.2")
    let rtOnly = try JSONDecoder().decode(Ratings.self, from: Data(#"{"imdb": null, "rottenTomatoes": "89%"}"#.utf8))
    #expect(ratingParts(rtOnly).isEmpty)
    #expect(ratingParts(Ratings(imdb: nil)).isEmpty)
    #expect(ratingParts(nil).isEmpty)
}

// Subtitle notes (plans/m13-turkcealtyazi.md 4.2).

private func playback(_ extra: String) throws -> PlaybackSnapshot {
    let json = #"{"phase": "ready", "url": "http://127.0.0.1:1/s/t/a.mkv", "key": null, "filename": null, "sizeBytes": null, "resumeMs": null,"#
        + #" "attempts": [], "subtitles": [], "subtitlesPending": false, "next": null, "bingeGroup": null, "nextAtMs": null"# + extra + "}"
    return try JSONDecoder().decode(PlaybackSnapshot.self, from: Data(json.utf8))
}

@Test func subtitleNotesDecodeWithoutTheFieldAndWithAnUnknownProblem() throws {
    // An older core sends none: an empty list.
    #expect(try playback("").notes == [])
    let notes = try playback(#", "subtitleNotes": [{"source": "TurkceAltyazi.org", "problem": "blocked"}, {"source": "Subs", "problem": "somethingNew"}]"#).notes
    #expect(notes == [SubtitleNote(source: "TurkceAltyazi.org", problem: "blocked"), SubtitleNote(source: "Subs", problem: "somethingNew")])
    #expect(notes[1].words == "Subtitles from Subs did not load.")
}

// The start progress and the subtitle picker's fields (plans/player-polish.md 1.3, 2).

@Test func theStartAndTheTracksOwnNamesDecodeWithAndWithoutTheFields() throws {
    let old = try playback("")
    #expect(old.start == nil)
    let start = #"{"stage": "buffering", "peers": 3, "haveBytes": 4100000, "needBytes": 8400000, "bytesPerS": 1048576, "#
        + #""source": 2, "sources": 3, "progress": 0.42, "caption": "Trying source 2 of 3"}"#
    let new = try playback(#", "start": "# + start)
    #expect(new.start?.stage == "buffering" && new.start?.source == 2 && new.start?.needBytes == 8_400_000)
    #expect(new.start?.caption == "Trying source 2 of 3")
    let track = { (extra: String) in
        let json = #"{"url": "u", "lang": "tur", "label": null, "source": "S", "forced": false, "hearingImpaired": false"# + extra + "}"
        return try JSONDecoder().decode(SubtitleTrack.self, from: Data(json.utf8))
    }
    let before = try track("")
    #expect(before.nativeName == nil && before.fits == nil && before.release == nil)
    let now = try track(#", "nativeName": "Türkçe", "fits": true, "release": "Film.2020.1080p.WEB-FLUX-hi""#)
    #expect(now.nativeName == "Türkçe" && now.fits == true)
    #expect(now.release == "Film.2020.1080p.WEB-FLUX-hi")
}

@Test func foregroundSaysWhenTheStreamServerMoved() throws {
    let decode = { (json: String) in try JSONDecoder().decode(ForegroundReply.self, from: Data(json.utf8)) }
    // Moved: the URLs handed out before are dead, and the player opens its source again.
    #expect(try decode(#"{"serverMoved": true}"#).moved)
    #expect(try !decode(#"{"serverMoved": false}"#).moved)
    // An older core answers {}: nothing moved.
    #expect(try !decode("{}").moved)
}
