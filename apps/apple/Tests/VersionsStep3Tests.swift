import Foundation
import Testing

// plans/play-every-version.md Step 3 on Apple: the shared table of zero-playable states, the title's
// Play, the player's failure card, Choose another version from the core's clock, the stall the player
// tells the core of, the core's shortcuts and sections, and the words no viewer reads any more.

private let fixtures = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "Fixtures/versions")

private func versions(_ name: String) throws -> VersionsSnapshot {
    try JSONDecoder().decode(VersionsSnapshot.self, from: Data(contentsOf: fixtures.appending(path: "\(name).json")))
}

private func playback(_ name: String) throws -> PlaybackSnapshot {
    try JSONDecoder().decode(PlaybackSnapshot.self, from: Data(contentsOf: fixtures.appending(path: "\(name).json")))
}

private func empty(_ kind: String, key: String? = nil) -> EmptyVersions {
    EmptyVersions(kind: kind, count: 0, rows: [], key: key)
}

private func snapshot(_ empty: EmptyVersions?, pick: VersionRow? = nil, done: Bool = true) -> VersionsSnapshot {
    VersionsSnapshot(
        recommended: pick, rows: [], pendingAddons: [], slowAddons: [], failedAddons: [], done: done, frozen: false,
        conditions: nil, note: nil, empty: empty)
}

@Test func everyZeroPlayableStateOffersTheSharedTablesActionsBackLast() {
    #expect(VersionWords.emptyActs(empty("noSource")) == [.settingsAddons, .back])
    #expect(VersionWords.emptyActs(empty("unreachable")) == [.tryAgain, .back])
    #expect(VersionWords.emptyActs(empty("unreachable", key: "bt:kept")) == [.playFromDevice(key: "bt:kept"), .tryAgain, .back])
    // Choose a version is the title's and the player's: on Versions the rows are listed, each startable.
    #expect(VersionWords.emptyActs(empty("nothingPlaysHere")) == [.back])
    // No account call to action anywhere but Settings (the owner's rule): needsAccount offers Back alone.
    for kind in ["kidsNothing", "noResults", "needsAccount", "somethingNew"] {
        #expect(VersionWords.emptyActs(empty(kind)) == [.back], "\(kind)")
    }
    #expect(VersionWords.emptyActs(empty("unreachable", key: "k")).map(VersionWords.emptyActWord) == ["Play from this device", "Try again", "Back"])
    #expect(VersionWords.emptyLine("needsAccount") == "None of these plays on this device")
    #expect(VersionWords.emptyLine("notStarted") == "This didn't start")
}

@Test func aDimmedRowSaysWhyInOneLineWithItsIcon() throws {
    #expect(VersionWords.dimmedStatus("needsAccount")?.text == "Won't play here")
    #expect(VersionWords.dimmedStatus("needsAccount")?.glyph == "ban")
    #expect(VersionWords.dimmedStatus("nothingPlaysHere")?.text == "Won't play here")
    #expect(VersionWords.dimmedStatus("nothingPlaysHere")?.glyph == "ban")
    #expect(VersionWords.dimmedStatus(nil) == nil)
    #expect(VersionWords.dimmedStatus("unreachable") == nil)
    // The rows an empty state keeps are listed, the count beside them, no zero beside a bare line.
    let locked = try versions("needs-account")
    #expect(VersionWords.sections(locked).flatMap(\.rows).map(\.key) == ["rd:1", "rd:2", "rd:3"])
    #expect(VersionWords.count(locked) == "3 versions")
    #expect(VersionWords.count(snapshot(empty("noResults"))) == nil)
    #expect(VersionWords.sections(snapshot(empty("noResults"))).isEmpty)
}

@Test func theTitlesPlayFollowsTheSharedTable() throws {
    // Still searching: the player opens at once with no key; a pick plays at once.
    #expect(TitlePlay.of(nil) == .player(key: nil))
    #expect(TitlePlay.of(snapshot(nil, done: false)) == .player(key: nil))
    #expect(TitlePlay.of(try versions("shortcuts")) == .player(key: "bt:pick"))
    // No source in reach: the source kept here plays, else Versions holds the line and Try again.
    #expect(TitlePlay.of(snapshot(empty("unreachable", key: "bt:kept"))) == .player(key: "bt:kept"))
    #expect(TitlePlay.of(snapshot(empty("unreachable"))) == .versions)
    for kind in ["noSource", "nothingPlaysHere"] {
        #expect(TitlePlay.of(snapshot(empty(kind))) == .versions, "\(kind)")
    }
    // Back alone would be a dead end: Play stays, inert.
    for kind in ["kidsNothing", "noResults", "needsAccount"] {
        #expect(TitlePlay.of(snapshot(empty(kind))) == .inert, "\(kind)")
    }
}

@Test func thePlayersFailureCardFollowsTheSharedTableAndNeverOffersAnAccount() throws {
    // The budget ran out: the core's line, Try again, Choose another version, Back.
    let notStarted = try playback("playback-not-started")
    #expect(PlayerLayout.failureHeadline(empty: notStarted.empty, offer: false) == "This didn't start")
    #expect(PlayerLayout.failureActions(empty: "notStarted", offer: false, canChoose: true) == [.retry, .choose, .back])
    #expect(PlayerLayout.failureActions(empty: "unreachable", offer: false, kept: true, canChoose: true) == [.playFromDevice, .retry, .back])
    #expect(PlayerLayout.failureActions(empty: "unreachable", offer: false, canChoose: true) == [.retry, .back])
    #expect(PlayerLayout.failureActions(empty: "nothingPlaysHere", offer: false, canChoose: true) == [.choose, .back])
    // A hand pick that failed: the ranking's next instead, another version, Back.
    #expect(PlayerLayout.failureActions(empty: "nothingPlaysHere", offer: true, canChoose: true) == [.offer, .choose, .back])
    // An older core names no state: Try again first.
    #expect(PlayerLayout.failureActions(empty: nil, offer: false, canChoose: false) == [.retry, .back])
    // An older core's noRoom (gone, plans/no-space-warnings.md) reads as any kind this build does not know.
    #expect(PlayerLayout.failureActions(empty: "noRoom", offer: false, canChoose: true)
        == PlayerLayout.failureActions(empty: "somethingNew", offer: false, canChoose: true))
    #expect(TitlePlay.of(snapshot(empty("noRoom"))) == TitlePlay.of(snapshot(empty("somethingNew"))))
    #expect(PlayerLayout.failureHeadline(empty: nil, offer: true) == "This version could not be played")
    #expect(PlayerLayout.failureLabel(.offer, offer: "720p WEB") == "Play 720p WEB instead")
    #expect(PlayerLayout.failureLabel(.choose, offer: nil) == "Choose another version")
    #expect(PlayerLayout.failureLabel(.versions, offer: nil) == "Choose a version")
    #expect(PlayerLayout.failureLabel(.playFromDevice, offer: nil) == "Play from this device")
    #expect(PlayerLayout.failureFocus([.retry, .choose, .back]) == .retry)
    // Play opened the player while versions were found and none could start: Versions holds the way on.
    // Play opened the player while versions were found, and the core's view ended empty: the table.
    #expect(PlayerLayout.noPickActions(empty("noSource"), canChoose: true) == [.settingsAddons, .back])
    #expect(PlayerLayout.noPickActions(empty("unreachable", key: "bt:kept"), canChoose: true) == [.playFromDevice, .retry, .back])
    #expect(PlayerLayout.noPickActions(empty("unreachable"), canChoose: true) == [.retry, .back])
    #expect(PlayerLayout.noPickActions(empty("nothingPlaysHere"), canChoose: true) == [.versions, .back])
    for kind in ["kidsNothing", "noResults", "needsAccount"] {
        #expect(PlayerLayout.noPickActions(empty(kind), canChoose: true) == [.back], "\(kind)")
    }
    #expect(PlayerLayout.noPickHeadline(empty("needsAccount")) == "None of these plays on this device")
    #expect(PlayerLayout.noPickHeadline(nil) == "No source has this yet")
    // The attempt history is plain words, never the addon's name.
    #expect(PlayerLayout.failureLines(notStarted.attempts) == ["No peer sent any data · 5 sources", "The swarm had no seeders"])
    // The source kept here: the empty state's, else the row the core marks on this device.
    #expect(PlayerLayout.kept(snapshot(empty("unreachable", key: "bt:kept"))) == "bt:kept")
    #expect(PlayerLayout.kept(try versions("shortcuts")) == nil)
}

@Test func chooseAnotherVersionShowsOnlyWhileTheCoreSaysSo() throws {
    let loading = try playback("playback-choose-another")
    #expect(loading.chooseAnother == true && loading.start?.caption == "Finding peers, 3 so far")
    #expect(PlayerLayout.offersAnother(loading, status: .waiting(words: true)))
    // Over a stalled frame too.
    #expect(PlayerLayout.offersAnother(loading, status: .waiting(words: false)))
    // Never on a card, and never before the core's clock says so (no app timer).
    #expect(!PlayerLayout.offersAnother(loading, status: .failed))
    #expect(!PlayerLayout.offersAnother(try playback("playback-not-started"), status: .waiting(words: true)))
    #expect(!PlayerLayout.offersAnother(nil, status: .waiting(words: true)))
    // The loading screen's words: the pick being found, then the core's stage caption, no addon name.
    #expect(PlayerLayout.waitingWords(nil, notice: nil, findingPick: true) == "Finding the best version")
    #expect(PlayerLayout.waitingWords(loading, notice: nil) == "Finding peers, 3 so far")
}

@Test func aStallIsToldWhenItStartsMidPlayAndWhenItEnds() {
    var watch = StallWatch()
    // The start's own wait is no stall, nor a wait before a second of steady playing.
    #expect(watch.see(playing: false, waiting: true, position: 0) == nil)
    #expect(watch.see(playing: true, waiting: false, position: 0) == nil)
    #expect(watch.see(playing: true, waiting: false, position: 0.5) == nil)
    #expect(watch.see(playing: false, waiting: true, position: 0.5) == nil)
    #expect(watch.see(playing: true, waiting: false, position: 0.5) == nil)
    #expect(watch.see(playing: true, waiting: false, position: 2) == nil)
    // Mid-play: it starts once, and playing on ends it.
    #expect(watch.see(playing: false, waiting: true, position: 2) == true)
    #expect(watch.see(playing: false, waiting: true, position: 2) == nil)
    #expect(watch.see(playing: true, waiting: false, position: 2) == false)
    // Still mid-play: a wait right after is a stall again.
    #expect(watch.see(playing: false, waiting: true, position: 2.1) == true)
    // A pause ends it; the wait after the pause is not a stall.
    #expect(watch.see(playing: false, waiting: false, position: 2.1) == false)
    #expect(watch.see(playing: false, waiting: true, position: 2.1) == nil)
    #expect(watch.see(playing: true, waiting: false, position: 2.1) == nil)
    #expect(watch.see(playing: true, waiting: false, position: 3.2) == nil)
    #expect(watch.see(playing: false, waiting: true, position: 3.2) == true)
    // A seek ends it; the wait after a seek is not a stall (bench EW-3: 17.2 s after a seek).
    #expect(watch.interrupted() == false)
    #expect(watch.see(playing: false, waiting: true, position: 600) == nil)
    #expect(watch.interrupted() == nil)
    let command = StallWatch.command(true, position: 61.25)
    #expect(command["type"] as? String == "stalled" && command["stalled"] as? Bool == true && command["positionMs"] as? Int64 == 61_250)
}

@Test func theShortcutFixtureKeepsTheCoresPromises() throws {
    // What the core promises of `smaller` and `sharper` (2f), so a fixture never shows a page the core cannot.
    let s = try versions("shortcuts")
    let pick = try #require(s.recommended)
    let smaller = try #require(s.smaller), sharper = try #require(s.sharper)
    let height = ["4K": 2160, "1440p": 1440, "1080p": 1080, "720p": 720, "SD": 480]
    #expect(!s.rows.contains { $0.key == pick.key }, "the pick is never in the list")
    #expect(try #require(smaller.sizeBytes) * 2 < (try #require(pick.sizeBytes)), "smaller is under half the pick's size")
    #expect(try #require(height[sharper.quality]) > (try #require(height[pick.quality])), "sharper is sharper than the pick")
    #expect(smaller.key != pick.key && sharper.key != pick.key)
    // The shortcuts stay in their sections, and every section key is a row.
    let keys = Set(s.rows.map(\.key))
    #expect(keys.contains(smaller.key) && keys.contains(sharper.key))
    #expect(try #require(s.sections).allSatisfy { $0.keys.allSatisfy(keys.contains) })
}

@Test func theListFollowsTheCoresSectionsAndTheShortcutsSayWhatTheyGive() throws {
    let s = try versions("shortcuts")
    let sections = VersionWords.sections(s)
    #expect(sections.map(\.title) == ["4K", "1080p", "720p and below"])
    #expect(sections.map { $0.rows.map(\.key) } == [["bt:4k"], ["bt:1080b", "bt:1080c"], ["bt:720"]])
    #expect(VersionWords.shortcuts(s).map(\.label) == ["Smaller file: 720p, 1.5 GB", "Sharper: 4K, 12 GB"])
    // Never the pick as a shortcut.
    var same = s
    same.smaller = s.recommended
    #expect(VersionWords.shortcuts(same).map(\.row.key) == ["bt:4k"])
    #expect(VersionWords.count(s) == "5 versions")
    #expect(VersionWords.status(s) == nil)
    #expect(VersionWords.status(nil) == "Finding versions")
    #expect(VersionWords.status(snapshot(nil, pick: s.recommended, done: false)) == "Still looking for more versions")
    #expect(VersionWords.showAll(12) == "Show all 12")
    // An older core without sections: one untitled list.
    var old = s
    old.sections = nil
    #expect(VersionWords.sections(old).map(\.title) == [nil])
}

@Test func overThePlayerTheRowsHoldTheirOrderAndNewOnesComeAfter() {
    let first = [VersionSection(quality: "fhd", keys: ["a", "b"]), VersionSection(quality: "hd", keys: ["c"])]
    let shown = VersionWords.hold(first, shown: [:])
    #expect(shown == ["fhd": ["a", "b"], "hd": ["c"]])
    // The core re-ranks b over a and finds d and e: a and b keep their places, the new ones follow.
    let next = [VersionSection(quality: "fhd", keys: ["d", "b", "a"]), VersionSection(quality: "hd", keys: ["e", "c"])]
    #expect(VersionWords.hold(next, shown: shown) == ["fhd": ["a", "b", "d"], "hd": ["c", "e"]])
    // A row the core no longer lists goes.
    #expect(VersionWords.hold([VersionSection(quality: "fhd", keys: ["b"])], shown: shown) == ["fhd": ["b"]])
}

@Test func theNewSnapshotFieldsDecodeAndOlderCoresStillDo() throws {
    let s = try versions("unreachable")
    #expect(s.empty?.kind == "unreachable" && s.empty?.key == "bt:kept" && s.sections == [])
    let old = try JSONDecoder().decode(PlaybackSnapshot.self, from: Data(#"{"phase":"failed","url":null,"key":"k","filename":null,"sizeBytes":null,"resumeMs":null,"attempts":[],"subtitles":[],"subtitlesPending":false,"next":null,"bingeGroup":null}"#.utf8))
    #expect(old.chooseAnother == nil && old.empty == nil)
}

/// String literals of a Swift source, comments left out.
private func literals(_ source: String) -> [String] {
    var found: [String] = []
    for line in source.split(separator: "\n", omittingEmptySubsequences: false) {
        var current: String?
        var escaped = false
        var previous: Character?
        for c in line {
            if var text = current {
                if escaped { escaped = false; text.append(c) } else if c == "\\" { escaped = true; text.append(c) } else if c == "\"" {
                    found.append(text)
                    current = nil
                    previous = c
                    continue
                } else { text.append(c) }
                current = text
            } else if c == "\"" {
                current = ""
            } else if c == "/", previous == "/" {
                break
            }
            previous = c
        }
    }
    return found
}

@Test func noViewerReadsTheOldDeadEndWordsOnVersionsTheTitleOrThePlayer() throws {
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    // No free space or storage warning anywhere (plans/no-space-warnings.md): the same list as the web's.
    let space = ["free space", "{free}", "nearly full", "device is full", "storage full", "storage is full", "no room",
                 "for this storage", "not enough space", "not enough room", "not enough storage", "out of space",
                 "low on storage", "free some storage"]
    let banned = ["no playable", "not measured", "estimated", "add an account", "show them", "hidden (", "via "] + space
    // The shared words too: every string but the rules text, by its path. The dead keys that wait for every
    // app to read `empty` (plan Step 3 item 6).
    let pending: Set = [
        "blocked.nothingPlaysHere.action", "blocked.showRows",
    ]
    let json = try JSONSerialization.jsonObject(with: Data(contentsOf: app.appending(path: "../../shared/stream-choice-words.json")))
    var hits: [String] = []
    func walk(_ value: Any, _ path: String) {
        if let text = value as? String, banned.contains(where: text.lowercased().contains) { hits.append(path) }
        if let list = value as? [Any] { for (i, x) in list.enumerated() { walk(x, "\(path)[\(i)]") } }
        if let map = value as? [String: Any] { for (k, x) in map where !(path.isEmpty && k == "about") { walk(x, path.isEmpty ? k : "\(path).\(k)") } }
    }
    walk(json, "")
    #expect(Set(hits).subtracting(pending).isEmpty, "\(hits)")
    // A pending key no longer said leaves the list.
    #expect(pending.subtracting(hits).isEmpty, "\(pending.subtracting(hits))")
    var scanned = 0
    for folder in ["Versions", "Title", "Player"] {
        for file in try FileManager.default.contentsOfDirectory(atPath: app.appending(path: folder).path()) where file.hasSuffix(".swift") {
            let source = try String(contentsOf: app.appending(path: "\(folder)/\(file)"), encoding: .utf8)
            for text in literals(source) {
                for word in banned {
                    #expect(!text.lowercased().contains(word), "\(folder)/\(file): \"\(text)\" holds \"\(word)\"")
                }
            }
            scanned += 1
        }
    }
    #expect(scanned >= 20)
    // Space words nowhere else a viewer reads either: every folder of the app.
    let all = try FileManager.default.subpathsOfDirectory(atPath: app.path()).filter { $0.hasSuffix(".swift") }
    for file in all {
        for text in literals(try String(contentsOf: app.appending(path: file), encoding: .utf8)) {
            for word in space {
                #expect(!text.lowercased().contains(word), "\(file): \"\(text)\" holds \"\(word)\"")
            }
        }
    }
    #expect(all.count > scanned)
    // The scanner itself: it finds a literal and leaves a comment alone.
    #expect(literals(#"let a = "Add an account" // "Show them""#) == ["Add an account"])
}

@Test func thePlayerWiresTheStallTheCoverAndThePickThroughTheRules() throws {
    // Read from the source: the model needs a player and a core.
    let player = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Player")
    let model = try String(contentsOf: player.appending(path: "PlayerModel.swift"), encoding: .utf8)
    let screen = try String(contentsOf: player.appending(path: "PlayerScreen.swift"), encoding: .utf8)
    // Every state change of the player reaches the stall watch, unless Versions covers it.
    #expect(try body(of: "private func observePlayer()", in: model)
        .contains("if !self.covered { self.sendStall(self.stall.see(playing: self.playing, waiting: self.waiting, position: self.position)) }"))
    #expect(try body(of: "private func tick(", in: model).contains("if playing { sendStall(stall.see(playing: true, waiting: false, position: position)) }"))
    // A seek, another version and another source end a stall.
    #expect(try body(of: "func seek(to seconds: Double)", in: model).contains("sendStall(stall.interrupted())"))
    #expect(try body(of: "func switchVersion(_ key: String)", in: model).contains("sendStall(stall.interrupted())"))
    #expect(try body(of: "private func apply(", in: model).contains("sendStall(stall.interrupted())\n            load(url"))
    // Under Versions nothing plays: a source that comes in loads paused.
    #expect(try body(of: "private func load(", in: model).contains("if holdPause || covered { holdPause = false } else { player.play() }"))
    #expect(try body(of: "func cover()", in: model).contains("player.pause()"))
    // Play pressed while versions are found: the player waits for the pick itself.
    // No app clock before the pick: the wait ends with the core's view, and never in a silent spinner.
    let run = try body(of: "func run() async", in: model)
    #expect(run.contains("versions.settledPick(tapped: route.tapped, limit: nil)"))
    #expect(run.contains("guard let versions else {\n                noPick = true"))
    #expect(!screen.contains("No version was ready in time"))
    // Under the cover the player is inert and the cover is modal; the player has no swipe back (its back
    // button is hidden, which turns the edge swipe off) while Versions covers it or ever.
    #expect(screen.contains(".disabled(showVersions)\n            .accessibilityHidden(showVersions)"))
    #expect(screen.contains(".accessibilityAddTraits(.isModal)"))
    #expect(screen.contains(".navigationBarBackButtonHidden()"))
    #expect(try body(of: "private func another(", in: screen).contains(".allowsHitTesting(shown)"))
    // Versions opens over the player, never as a pushed page that would close it.
    #expect(screen.contains("chooseVersion: model.versions == nil ? nil : { openVersions() }"))
    #expect(try body(of: "private func openVersions()", in: screen).contains("model.cover()"))
    #expect(try body(of: "private func closeVersions()", in: screen).contains("model.uncover()"))
    #expect(!screen.contains("navigationDestination(isPresented: $showVersions)"))
    #expect(try body(of: "private func another(", in: screen).contains("PlayerLayout.offersAnother(model.playback, status: status)"))
}

/// A row from a base with `json`'s fields over it (`json` starts with ", ").
private func row(_ json: String) throws -> VersionRow {
    let base = #"{"key": "k", "quality": "1080p", "features": ["HDR10", "Atmos"], "sizeBytes": 12884901888, "status": {"kind": "torrent", "seeds": 1204}, "via": "Sample Source", "releaseName": "Sintel.2010.1080p.mkv", "languages": ["en"], "reasons": [], "group": "best", "failoverCount": 1}"#
    var fields = try #require(JSONSerialization.jsonObject(with: Data(base.utf8)) as? [String: Any])
    let over = try #require(JSONSerialization.jsonObject(with: Data(("{" + json.dropFirst(2) + "}").utf8)) as? [String: Any])
    fields.merge(over) { _, new in new }
    return try JSONDecoder().decode(VersionRow.self, from: JSONSerialization.data(withJSONObject: fields))
}

@Test func theCardIsItsKickerBadgesAndOneWhyLine() throws {
    #if os(macOS)
    #expect(VersionWords.kicker == "Best for this computer")
    #endif
    let pick = try row(#", "line": {"kind": "quick", "seeders": 412, "sizeBytes": 2147483648}"#)
    #expect(VersionWords.badges(pick) == [.init(text: "1080p", look: .filled), .init(text: "HDR", look: .outlined), .init(text: "Dolby Atmos", look: .plain)])
    #expect(VersionWords.why(pick) == "Starts quickly: shared by 412 people, 2 GB")
    // One video and one audio format at most, the best of each.
    let dv = try row(#", "features": ["Dolby Vision", "HDR10", "7.1", "5.1"]"#)
    #expect(VersionWords.badges(dv).map(\.text) == ["1080p", "Dolby Vision", "7.1"])
    // A quality the release does not state gets no badge: its section ("Other") says it. A source in its place is one.
    #expect(VersionWords.badges(try row(#", "features": [], "quality": "Quality not stated""#)).isEmpty)
    #expect(VersionWords.badges(try row(#", "features": ["HDR10"], "quality": "Quality not stated""#)).map(\.text) == ["HDR"])
    #expect(VersionWords.badges(try row(#", "features": [], "quality": "Unknown""#)).isEmpty)
    #expect(VersionWords.badges(try row(#", "features": [], "quality": "WEB-DL""#)).map(\.text) == ["WEB-DL"])
    // Said with no empty part; and the badge line keeps its height with none (an unseen badge).
    let none = try row(#", "features": [], "quality": "Quality not stated", "line": {"kind": "quick"}"#)
    #expect(VersionWords.cardSpoken(none) == "\(VersionWords.kicker). \(VersionWords.why(none))")
    let page = try String(contentsOf: URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        .appending(path: "../Marquee/Versions/VersionsScreen.swift"), encoding: .utf8)
    let badgeRow = try body(of: "private struct BadgeRow", in: page)
    #expect(badgeRow.contains("if badges.isEmpty { badge(.init(text: \"4K\", look: .filled)).hidden() }"))
    // No badge is no VoiceOver stop; an outlined badge is as tall as a filled one (1 pt each way).
    #expect(badgeRow.contains(".accessibilityHidden(badges.isEmpty)"))
    #expect(badgeRow.contains(".padding(.horizontal, 5).padding(.vertical, 1)") && badgeRow.contains(".padding(.horizontal, 6).padding(.vertical, 1)"))
    #expect(try body(of: "private struct PickCard", in: page).contains(".accessibilityLabel(VersionWords.cardSpoken(row, kicker: kicker))"))
}

@Test func theCardSaysOnThisDeviceAloneForTheVersionKeptHere() throws {
    let kept = try row(#", "onDevice": "On this device", "line": {"kind": "quick", "seeders": 22, "sizeBytes": 2147483648}"#)
    // One idea, as the web and Android: the row keeps the line's numbers for its details.
    #expect(VersionWords.why(kept) == "On this device")
    #expect(VersionWords.cardSpoken(kept) == "\(VersionWords.kicker). 1080p, HDR, Dolby Atmos. On this device")
    #expect(VersionWords.details(kept).contains("Shared by 22 people"))
}

@Test func aRowIsItsBadgesAndOneQuietLineWithItsSizeFirst() throws {
    // A row that plays as it should: its size, then its source in plain words.
    #expect(VersionWords.rowWords(try row(#", "line": {"kind": "quick", "seeders": 1204}"#)) == "12 GB, shared by 1,204 people")
    #expect(VersionWords.rowWords(try row(#", "status": {"kind": "ready", "service": "TorBox"}, "line": {"kind": "ready", "service": "TorBox"}"#)) == "12 GB, ready now")
    #expect(VersionWords.rowWords(try row(#", "status": {"kind": "direct"}, "line": {"kind": "quick"}"#)) == "12 GB, direct link")
    // A warning takes the source's place, in lower case after the size, with a capital without one.
    #expect(VersionWords.rowWords(try row(#", "line": {"kind": "few", "seeders": 5}"#)) == "12 GB, may start slowly")
    #expect(VersionWords.rowWords(try row(#", "sizeBytes": null, "line": {"kind": "didNotPlay"}"#)) == "Did not play on this device")
    // Dimmed: why it may not play.
    #expect(VersionWords.rowWords(try row(#", "line": {"kind": "quick"}"#), dimmed: "Needs an account") == "12 GB, needs an account")
    // The details, laid out always, shown on focus or hover: the release, the line's numbers, languages and sources.
    #expect(VersionWords.details(try row(#", "line": {"kind": "few", "seeders": 5}"#)) == "Sintel.2010.1080p.mkv \u{B7} Shared by 5 people \u{B7} English, 2 sources")
    // Its language tag only in another language than the viewer's; never the addon in what is read out.
    let other = try row(#", "group": "otherLanguages", "languages": ["tr"], "line": {"kind": "quick"}"#)
    #expect(VersionWords.languageTag(other) == "Turkish")
    #expect(VersionWords.languageTag(try row(#", "line": {"kind": "quick"}"#)) == nil)
    #expect(VersionWords.rowSpoken(other) == "1080p, HDR, Dolby Atmos, Turkish, 12 GB, shared by 1,204 people")
    #expect(!VersionWords.rowSpoken(other).contains("Sample Source"))
}

@Test func thePageIsFlatWithNoTableAndPlainHeadings() throws {
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    let page = try String(contentsOf: app.appending(path: "Versions/VersionsScreen.swift"), encoding: .utf8)
    let player = try String(contentsOf: app.appending(path: "Player/PlayerScreen.swift"), encoding: .utf8)
    // No glass and no edge on the cards, the failure card included; no column table.
    #expect(!page.contains(".glass("))
    #expect(!page.contains("ColumnHeadings") && !page.contains("Text(caps:"))
    let components = try String(contentsOf: app.appending(path: "Theme/Components.swift"), encoding: .utf8)
    #expect(try body(of: "func flatCard()", in: components).contains(".fill(Theme.Palette.surface)"))
    #expect(try body(of: "private func card(", in: player).contains(".fill(Theme.Palette.surface)"))
    #expect(!(try body(of: "private func card(", in: player)).contains(".glass("))
    // A dimmed row's badges take the muted tone with its words, never a faded row.
    #expect(try body(of: "private struct RowFace", in: page).contains("tone: dimmed == nil ? Theme.Palette.text : Theme.Palette.muted"))
    #expect(try body(of: "private var rowButton", in: page).contains("RowFace(row: row, dimmed: dimmed)"))
    #expect(!(try body(of: "private struct VersionRowView", in: page)).contains(".opacity(0.") )
    // The details line is always laid out and only fades: no height change on focus or hover.
    #expect(try body(of: "private struct VersionRowView", in: page).contains(".opacity(hovered || focused ? 1 : 0)"))
}

@Test func onTheIPhoneOneTapPlaysAndTheInfoButtonOpensTheDetails() throws {
    let page = try String(contentsOf: URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        .appending(path: "../Marquee/Versions/VersionsScreen.swift"), encoding: .utf8)
    let row = try body(of: "private struct VersionRowView", in: page)
    // One tap plays: the row is one button whose action is Play; the info button is its sibling.
    #expect(try body(of: "private var rowButton", in: row).hasPrefix("{\n        Button(action: play) {"))
    #expect(row.contains("HStack(spacing: 0) {\n            rowButton\n            if touch {"))
    // Wherever there is touch (an iPad too), the info button; one VoiceOver stop per row, so the button is
    // hidden from it and the row carries Details. The sheet is modal with its own Play; VoiceOver goes back
    // to the row when it closes.
    #expect(try body(of: "private var touch: Bool", in: row).contains("#if os(iOS)\n        true"))
    #expect(row.contains(".accessibilityHidden(true)\n                .sheet(isPresented: $sheet, onDismiss: { rowFocused = true })"))
    #expect(row.contains(".accessibilityFocused($rowFocused)"))
    let sheet = try body(of: "private struct DetailsSheet", in: page)
    #expect(sheet.contains("Button(action: play)") && sheet.contains(".accessibilityAddTraits(.isModal)"))
    #expect(sheet.contains("VersionWords.detailLines(row, dimmed: dimmed?.text)"))
    // Its top is what the row shows (badges, language tag, size and warning), then the seeders, the file, the language.
    #expect(sheet.contains("RowFace(row: row, dimmed: dimmed, big: true)"))
    let face = try body(of: "private struct RowFace", in: page)
    #expect(face.contains("VersionWords.badges(row)") && face.contains("VersionWords.languageTag(row)") && face.contains("RowLine(row: row, dimmed: dimmed)"))
    let r = try JSONDecoder().decode(VersionRow.self, from: Data(#"{"key": "k", "quality": "720p", "features": [], "sizeBytes": 1, "status": {"kind": "torrent", "seeds": 5}, "via": "Sample Source", "releaseName": "Sintel.720p.mp4", "languages": ["en"], "reasons": [], "group": "best", "failoverCount": 0, "line": {"kind": "few", "seeders": 5}}"#.utf8))
    #expect(VersionWords.detailLines(r) == ["Shared by 5 people", "Sintel.720p.mp4", "English"])
}

@Test func anInertEpisodeOrHeroOpensNoBackOnlyPage() throws {
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    let title = try String(contentsOf: app.appending(path: "Title/TitleScreen.swift"), encoding: .utf8)
    let home = try String(contentsOf: app.appending(path: "Home/HomeScreen.swift"), encoding: .utf8)
    #expect(try body(of: "private func play(_ episode: Episode)", in: title).contains("case .inert: break"))
    #expect(try body(of: "private func playHero(", in: home).contains("case .inert: break"))
}

@Test func theTitlesLineUnderPlayIsTheCoresWarningOnly() throws {
    // Searching or with a pick: nothing under Play, never "Finding versions", badges or a size.
    #expect(titleWarning(nil) == nil)
    #expect(titleWarning(snapshot(nil, done: false)) == nil)
    #expect(titleWarning(try versions("shortcuts")) == nil)
    #expect(titleWarning(snapshot(empty("kidsNothing"))) == "There's nothing here for you to watch")
    let title = try String(contentsOf: URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        .appending(path: "../Marquee/Title/TitleScreen.swift"), encoding: .utf8)
    let line = try body(of: "struct VersionLine: View", in: title)
    #expect(line.contains("titleWarning(versions)"))
    #expect(!line.contains("Finding versions") && !line.contains("ProgressView") && !line.contains("rowSpoken"))
    #expect(try body(of: "private struct VersionNote", in: title).contains("titleWarning(versions)"))
}

@Test func everySizeIsTheSharedOneTheMenuIncluded() throws {
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    // One size function on every screen that names a version: 1024-based, as the shared words say.
    for folder in ["Versions", "Title", "Player"] {
        for file in try FileManager.default.contentsOfDirectory(atPath: app.appending(path: folder).path()) where file.hasSuffix(".swift") {
            let source = try String(contentsOf: app.appending(path: "\(folder)/\(file)"), encoding: .utf8)
            #expect(!source.contains("ByteCountFormatter"), "\(folder)/\(file)")
        }
    }
    let controls = try String(contentsOf: app.appending(path: "Player/PlayerControls.swift"), encoding: .utf8)
    #expect(controls.contains("Choice(text: VersionWords.rowSpoken(row), selected: row.key == playingKey)"))
    let row = try JSONDecoder().decode(VersionRow.self, from: Data(#"{"key": "k", "quality": "720p", "features": [], "sizeBytes": 1610612736, "status": {"kind": "direct"}, "via": "Sample Source", "releaseName": "A.mp4", "languages": [], "reasons": [], "group": "best", "failoverCount": 0, "line": {"kind": "quick"}}"#.utf8))
    #expect(VersionWords.rowSpoken(row) == "720p, 1.5 GB, direct link")
}

@Test func voiceOverReadsARowsDetailsAndThePhoneRowHasADetailsAction() throws {
    let page = try String(contentsOf: URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        .appending(path: "../Marquee/Versions/VersionsScreen.swift"), encoding: .utf8)
    let row = try body(of: "private struct VersionRowView", in: page)
    #expect(row.contains(".accessibilityValue(VersionWords.details(row))"))
    #expect(row.contains("if touch { Button(StreamChoice.words.page.details) { sheet = true } }"))
    #expect(StreamChoice.words.page.details == "Details")
}

@Test func tryAgainReallyAsksAgainAndAFailedOpenEndsTheWait() throws {
    let unreachable = try versions("unreachable")
    var noKept = unreachable
    noKept.empty = EmptyVersions(kind: "unreachable", count: 0, rows: [], key: nil)
    // No source in reach and none kept: the wait ends with no version, so the card shows.
    #expect(PickWait.ends(snapshot: noKept, failed: false, settled: false))
    #expect(PickWait.key(noKept) == nil)
    #expect(PickWait.key(unreachable) == "bt:kept")
    // Try again: what landed before it is stale, so the player waits again (nothing to read yet) ...
    #expect(!PickWait.fresh(landed: 4, retriedAt: 4))
    #expect(!PickWait.ends(snapshot: nil, failed: false, settled: false))
    // ... and the view asked again finds a pick: the wait ends with it and the player starts it.
    #expect(PickWait.fresh(landed: 5, retriedAt: 4))
    let found = try versions("shortcuts")
    #expect(PickWait.ends(snapshot: found, failed: false, settled: false))
    #expect(PickWait.key(found) == "bt:pick")
    #expect(PickWait.fresh(landed: 0, retriedAt: nil))
    // A view that could not open ends the wait: the card says no source could be reached, with Try again.
    #expect(PickWait.ends(snapshot: nil, failed: true, settled: false))
    let failed = PlayerLayout.noPickState(nil, failed: true)
    #expect(PlayerLayout.noPickHeadline(failed) == "Can't reach any source")
    #expect(PlayerLayout.noPickActions(failed, canChoose: true) == [.retry, .back])
    // A real empty state wins over a later failure.
    #expect(PlayerLayout.noPickState(empty("needsAccount"), failed: true)?.kind == "needsAccount")
    // Still asking with a pick not yet settled: wait on.
    let json = try String(contentsOf: fixtures.appending(path: "shortcuts.json"), encoding: .utf8).replacingOccurrences(of: #""done": true"#, with: #""done": false"#)
    let asking = try JSONDecoder().decode(VersionsSnapshot.self, from: Data(json.utf8))
    #expect(!PickWait.ends(snapshot: asking, failed: false, settled: false))
    #expect(PickWait.ends(snapshot: asking, failed: false, settled: true))
    // The wiring: Try again marks what landed before as stale and asks the view again; the wait loop reads
    // PickWait; back from the card's Settings the versions are asked again.
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    let model = try String(contentsOf: app.appending(path: "Versions/VersionsModel.swift"), encoding: .utf8)
    let record = try String(contentsOf: app.appending(path: "Core/LiveRecord.swift"), encoding: .utf8)
    let player = try String(contentsOf: app.appending(path: "Player/PlayerModel.swift"), encoding: .utf8)
    let screen = try String(contentsOf: app.appending(path: "Player/PlayerScreen.swift"), encoding: .utf8)
    #expect(try body(of: "func retry()", in: model).contains("retriedAt = record.landed\n        record.retry()"))
    #expect(try body(of: "func settledPick(", in: model).contains("PickWait.ends(snapshot: snapshot, failed: failure != nil"))
    #expect(record.contains("case .updates(let values): if let last = values.last { snapshot = last }\n                    }\n                    landed += 1"))
    #expect(try body(of: "func retryPick()", in: player).contains("versions?.retry()"))
    #expect(screen.contains("if old != nil, new == nil, model.noPick { model.retryPick() }"))
    #expect(screen.contains("PlayerLayout.noPickState(model.versions?.snapshot?.empty, failed: model.versions?.failure != nil)"))
}

/// What the core really sends (apps/shared/fixtures, kept true by the core's own tests against the live views).
private func golden<T: Decodable>(_ name: String, as type: T.Type) throws -> T {
    let url = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../../shared/fixtures/\(name).json")
    return try JSONDecoder().decode(T.self, from: Data(contentsOf: url))
}

@Test func theCoresRealEmptyStatesReadAsTheSharedTable() throws {
    // Each zero-playable state as the core sends it: the title's Play, the Versions page's actions, its rows.
    let table: [(kind: String, play: TitlePlay, acts: [VersionWords.EmptyAct], coreAction: String)] = [
        ("noSource", .versions, [.settingsAddons, .back], "settingsAddons"),
        ("unreachable", .versions, [.tryAgain, .back], "tryAgain"),
        ("needsAccount", .inert, [.back], "none"),
        ("nothingPlaysHere", .versions, [.back], "chooseVersion"),
        ("kidsNothing", .inert, [.back], "none"),
        ("noResults", .inert, [.back], "none"),
    ]
    for row in table {
        let s = try golden("versions-empty-\(row.kind)", as: VersionsSnapshot.self)
        let empty = try #require(s.empty, "\(row.kind)")
        #expect(empty.kind == row.kind)
        #expect(TitlePlay.of(s) == row.play, "\(row.kind)")
        #expect(VersionWords.emptyActs(empty) == row.acts, "\(row.kind)")
        #expect(VersionWords.emptyLine(row.kind) != nil, "\(row.kind)")
        #expect(titleWarning(s) == VersionWords.emptyLine(row.kind), "\(row.kind)")
        // The core's own action for the state agrees with the table's first step (Choose a version is the title's).
        let json = try JSONSerialization.jsonObject(with: Data(contentsOf: URL(fileURLWithPath: #filePath).deletingLastPathComponent()
            .appending(path: "../../shared/fixtures/versions-empty-\(row.kind).json"))) as? [String: Any]
        #expect((json?["empty"] as? [String: Any])?["action"] as? String == row.coreAction, "\(row.kind)")
    }
    // The rows a state keeps are listed dimmed with why, and counted.
    let locked = try golden("versions-empty-needsAccount", as: VersionsSnapshot.self)
    #expect(VersionWords.sections(locked).flatMap(\.rows).count == 2)
    #expect(VersionWords.count(locked) == "2 versions")
    #expect(VersionWords.dimmedStatus(locked.empty?.kind)?.glyph == "ban")
    let foreign = try golden("versions-empty-nothingPlaysHere", as: VersionsSnapshot.self)
    #expect(VersionWords.sections(foreign).flatMap(\.rows).count == 1)
    #expect(VersionWords.dimmedStatus(foreign.empty?.kind)?.glyph == "ban")
    #expect(VersionWords.count(try golden("versions-empty-noSource", as: VersionsSnapshot.self)) == nil)
}

@Test func theCoresRealPlaybackEndsReadAsTheSharedTable() throws {
    let notStarted = try golden("playback-empty-notStarted", as: PlaybackSnapshot.self)
    #expect(PlayerLayout.failureHeadline(empty: notStarted.empty, offer: notStarted.offer != nil) == "This didn't start")
    #expect(PlayerLayout.failureActions(empty: notStarted.empty, offer: notStarted.offer != nil, canChoose: true) == [.retry, .choose, .back])
    let foreign = try golden("playback-empty-nothingPlaysHere", as: PlaybackSnapshot.self)
    #expect(PlayerLayout.failureActions(empty: foreign.empty, offer: foreign.offer != nil, canChoose: true) == [.choose, .back])
    // The attempt history never names the addon the core still sends as `via`.
    for p in [notStarted, foreign] {
        #expect(!PlayerLayout.failureLines(p.attempts).joined().contains("Test"))
    }
    // Over a stalled frame the core says Choose another version, and the player shows it.
    let stalled = try golden("playback-chooseAnother", as: PlaybackSnapshot.self)
    #expect(stalled.phase == "ready" && stalled.chooseAnother == true)
    #expect(PlayerLayout.offersAnother(stalled, status: PlayerLayout.status(phase: stalled.phase, cannotOpen: false, firstFrameShown: true, waiting: true)))
}

@Test func needsAccountShowsNoAccountCallToActionOnVersionsTheTitleOrThePlayer() throws {
    // The owner's rule: an account is added in Settings only. needsAccount offers Back alone everywhere.
    let words = [VersionWords.emptyLine("needsAccount"), VersionWords.dimmedStatus("needsAccount")?.text,
                 PlayerLayout.noPickHeadline(empty("needsAccount"))].compactMap { $0 }
        + VersionWords.emptyActs(empty("needsAccount")).map(VersionWords.emptyActWord)
        + PlayerLayout.noPickActions(empty("needsAccount"), canChoose: true).map { PlayerLayout.failureLabel($0, offer: nil) }
    for word in words {
        for banned in ["add an account", "settings", "account", "streaming service", "real-debrid", "torbox"] {
            #expect(!word.lowercased().contains(banned), "\(word)")
        }
    }
    #expect(VersionWords.emptyActs(empty("needsAccount")) == [.back])
    #expect(PlayerLayout.noPickActions(empty("needsAccount"), canChoose: true) == [.back])
    #expect(TitlePlay.of(snapshot(empty("needsAccount"))) == .inert)
    // No account page is reached from Versions or the player: DebridScreen opens from Settings alone.
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    for file in ["Versions/VersionsScreen.swift", "Player/PlayerScreen.swift", "Title/TitleScreen.swift"] {
        let source = try String(contentsOf: app.appending(path: file), encoding: .utf8)
        #expect(!source.contains("DebridScreen("), "\(file)")
    }
}

@Test func afterTheColonAWordInCapitalsStays() {
    #expect(VersionWords.lowerFirst("CAM copy") == "CAM copy")
    #expect(VersionWords.lowerFirst("HDR file, screen without HDR") == "HDR file, screen without HDR")
    #expect(VersionWords.lowerFirst("Shared by 3 people") == "shared by 3 people")
}

@Test func theSheetDropsEachPartItsFaceSaysAndKeepsTheRest() throws {
    let gib = 1_073_741_824
    func r(_ line: String, status: String = #"{"kind": "torrent", "seeds": 89}"#, size: Int = 4 * 1_073_741_824, onDevice: String? = nil) throws -> VersionRow {
        let kept = onDevice.map { #", "onDevice": "\#($0)""# } ?? ""
        return try JSONDecoder().decode(VersionRow.self, from: Data(#"{"key": "k", "quality": "1080p", "features": [], "sizeBytes": \#(size), "status": \#(status), "via": "Sample Source", "releaseName": "A.mkv", "languages": [], "reasons": [], "group": "best", "failoverCount": 0, "line": \#(line)\#(kept)}"#.utf8))
    }
    // The face says "4 GB, shared by 89 people": the account is said only here.
    #expect(VersionWords.sheetDetails(try r(#"{"kind": "should", "seeders": 89, "sizeBytes": \#(4 * gib), "service": "TorBox"}"#)) == "May be ready in your account")
    // A ready file: "2 GB, ready now" says both parts.
    #expect(VersionWords.sheetDetails(try r(#"{"kind": "ready", "service": null, "sizeBytes": \#(2 * gib)}"#, status: #"{"kind": "ready", "service": null}"#, size: 2 * gib)) == "")
    // On this device: the size limit stays, as the face does not say it.
    #expect(VersionWords.sheetDetails(try r(#"{"kind": "large", "sizeBytes": \#(4 * gib), "limitBytes": \#(2 * gib), "chosen": false}"#, status: #"{"kind": "direct"}"#, onDevice: "On this device")) == "4 GB; Momotus prefers files up to 2 GB")
    // A part is said only whole: "14 GB" does not say "4 GB".
    #expect(VersionWords.says("14 GB, shared by 89 people", "4 GB") == false)
    #expect(VersionWords.says("1.4 GB, shared by 89 people", "4 GB") == false)
    #expect(VersionWords.says("4 GB, shared by 89 people", "4 GB"))
}

/// A page as the core counts its kinds ("torrent:37"), with the kind it lists.
private func kinds(_ kinds: [(String, Int)], service: String? = nil, selected: String? = nil) throws -> VersionsSnapshot {
    var s = snapshot(nil, pick: try row(#", "key": "0""#))
    s.kinds = kinds.map { KindChip(kind: $0.0, service: $0.0 == "debrid" ? service : nil, count: $0.1) }
    s.selectedKind = selected
    return s
}

// The kind chips (plans/plugins-and-marketplace.md 4.2, 4.3): the apps render what the core counts and lists.
@Test func theKindChipsAreAllAndTheCoresKindsFromTwoKindsOn() throws {
    #expect(!VersionWords.showsKinds(try kinds([("torrent", 2)])))
    #expect(!VersionWords.showsKinds(nil))
    let three = try kinds([("torrent", 37), ("direct", 1), ("debrid", 1204)])
    #expect(VersionWords.showsKinds(three))
    let chips = VersionWords.kindChips(three)
    #expect(chips.map(\.label) == ["All", "Torrents", "Direct links", "Debrid"])
    #expect(chips.map(\.count) == [nil, "37", "1", "1,204"])
    #expect(chips[1].spoken == "Torrents, 37 versions")
    #expect(chips[2].spoken == "Direct links, 1 version")
    // A kind from a newer core is left out.
    #expect(VersionWords.kindChips(try kinds([("plugin", 2), ("direct", 1)])).count == 2)
}

@Test func theCardsKickerFollowsTheKindTheCoreLists() throws {
    let named = try kinds([("debrid", 2), ("direct", 1)], service: "Real-Debrid", selected: "debrid")
    #expect(VersionWords.kindChips(named)[1].label == "Real-Debrid")
    #expect(VersionWords.kindKicker(named) == "Best on Real-Debrid")
    #expect(VersionWords.kindKicker(try kinds([("debrid", 2), ("direct", 1)], selected: "debrid")) == "Best debrid link")
    #expect(VersionWords.kindKicker(try kinds([("torrent", 2)], selected: "torrent")) == "Best torrent")
    #expect(VersionWords.kindKicker(try kinds([("direct", 2)], selected: "direct")) == "Best direct link")
    #expect(VersionWords.kindKicker(try kinds([("direct", 2)])) == VersionWords.kicker)
    // A row from a core that names no kind reads as a direct link.
    #expect(try row(", \"features\": []").playsAs == "direct")
    #expect(try row(#", "kind": "torrent""#).playsAs == "torrent")
    // A chip asks the core, and leaving the page lists every kind again.
    let page = try String(contentsOf: URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        .appending(path: "../Marquee/Versions/VersionsScreen.swift"), encoding: .utf8)
    let chipRow = try body(of: "private var kindRow", in: page)
    #expect(chipRow.contains("model.setKind(chip.kind)") && chipRow.contains(".accessibilityLabel(chip.spoken)"))
    #expect(page.contains(".onDisappear { if asked != nil || snapshot?.selectedKind != nil { model.setKind(nil) } }"))
}
