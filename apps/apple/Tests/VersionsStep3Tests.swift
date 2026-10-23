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
    #expect(VersionWords.emptyActs(empty("needsAccount")) == [.settingsAccounts, .back])
    // Choose a version is the title's and the player's: on Versions the rows are listed, each startable.
    #expect(VersionWords.emptyActs(empty("nothingPlaysHere")) == [.back])
    for kind in ["noRoom", "kidsNothing", "noResults", "somethingNew"] {
        #expect(VersionWords.emptyActs(empty(kind)) == [.back], "\(kind)")
    }
    #expect(VersionWords.emptyActs(empty("needsAccount")).map(VersionWords.emptyActWord) == ["Settings", "Back"])
    #expect(VersionWords.emptyActs(empty("unreachable", key: "k")).map(VersionWords.emptyActWord) == ["Play from this device", "Try again", "Back"])
    #expect(VersionWords.emptyLine("needsAccount") == "These play through a streaming account")
    #expect(VersionWords.emptyLine("notStarted") == "This didn't start")
}

@Test func aDimmedRowSaysWhyInOneLineWithItsIcon() throws {
    #expect(VersionWords.dimmedStatus("needsAccount")?.text == "Needs an account")
    #expect(VersionWords.dimmedStatus("needsAccount")?.glyph == "lock")
    #expect(VersionWords.dimmedStatus("nothingPlaysHere")?.text == "Won't play here")
    #expect(VersionWords.dimmedStatus("nothingPlaysHere")?.glyph == "ban")
    #expect(VersionWords.dimmedStatus(nil) == nil)
    #expect(VersionWords.dimmedStatus("unreachable") == nil)
    // The rows an empty state keeps are listed, the count beside them, no zero beside a bare line.
    let locked = try versions("needs-account")
    #expect(VersionWords.sections(locked).flatMap(\.rows).map(\.key) == ["rd:1", "rd:2", "rd:3"])
    #expect(VersionWords.count(locked) == "3 versions")
    #expect(VersionWords.count(try versions("no-room")) == nil)
    #expect(VersionWords.sections(try versions("no-room")).isEmpty)
}

@Test func theTitlesPlayFollowsTheSharedTable() throws {
    // Still searching: the player opens at once with no key; a pick plays at once.
    #expect(TitlePlay.of(nil) == .player(key: nil))
    #expect(TitlePlay.of(snapshot(nil, done: false)) == .player(key: nil))
    #expect(TitlePlay.of(try versions("shortcuts")) == .player(key: "bt:pick"))
    // No source in reach: the source kept here plays, else Versions holds the line and Try again.
    #expect(TitlePlay.of(snapshot(empty("unreachable", key: "bt:kept"))) == .player(key: "bt:kept"))
    #expect(TitlePlay.of(snapshot(empty("unreachable"))) == .versions)
    for kind in ["noSource", "needsAccount", "nothingPlaysHere"] {
        #expect(TitlePlay.of(snapshot(empty(kind))) == .versions, "\(kind)")
    }
    // Back alone would be a dead end: Play stays, inert.
    for kind in ["noRoom", "kidsNothing", "noResults"] {
        #expect(TitlePlay.of(snapshot(empty(kind))) == .inert, "\(kind)")
    }
}

@Test func thePlayersFailureCardFollowsTheSharedTableAndNeverOffersAnAccount() throws {
    // The budget ran out: the core's line, Try again, Choose another version, Back.
    let notStarted = try playback("playback-not-started")
    #expect(PlayerLayout.failureHeadline(empty: notStarted.empty, offer: false) == "This didn't start")
    #expect(PlayerLayout.failureActions(empty: "notStarted", offer: false, canChoose: true) == [.retry, .choose, .back])
    #expect(PlayerLayout.failureActions(empty: "noRoom", offer: false, canChoose: true) == [.back])
    #expect(PlayerLayout.failureActions(empty: "unreachable", offer: false, kept: true, canChoose: true) == [.playFromDevice, .retry, .back])
    #expect(PlayerLayout.failureActions(empty: "unreachable", offer: false, canChoose: true) == [.retry, .back])
    #expect(PlayerLayout.failureActions(empty: "nothingPlaysHere", offer: false, canChoose: true) == [.choose, .back])
    // A hand pick that failed: the ranking's next instead, another version, Back.
    #expect(PlayerLayout.failureActions(empty: "nothingPlaysHere", offer: true, canChoose: true) == [.offer, .choose, .back])
    // An older core names no state: Try again first.
    #expect(PlayerLayout.failureActions(empty: nil, offer: false, canChoose: false) == [.retry, .back])
    #expect(PlayerLayout.failureHeadline(empty: nil, offer: true) == "This version could not be played")
    #expect(PlayerLayout.failureLabel(.offer, offer: "720p WEB") == "Play 720p WEB instead")
    #expect(PlayerLayout.failureLabel(.choose, offer: nil) == "Choose another version")
    #expect(PlayerLayout.failureLabel(.versions, offer: nil) == "Choose a version")
    #expect(PlayerLayout.failureLabel(.playFromDevice, offer: nil) == "Play from this device")
    #expect(PlayerLayout.failureFocus([.retry, .choose, .back]) == .retry)
    // Play opened the player while versions were found and none could start: Versions holds the way on.
    #expect(PlayerLayout.noPickActions("needsAccount", canChoose: true) == [.versions, .back])
    #expect(PlayerLayout.noPickActions(nil, canChoose: true) == [.versions, .back])
    #expect(PlayerLayout.noPickActions("noResults", canChoose: true) == [.back])
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
    #expect(VersionWords.shortcuts(s).map(\.label) == ["Smaller file: 720p, 1.61 GB", "Sharper: 4K, 12.88 GB"])
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
    let banned = ["no playable", "not measured", "add an account", "show them", "hidden (", "via "]
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
    #expect(try body(of: "func run() async", in: model).contains("versions.settledPick(tapped: route.tapped, limit: .seconds(30))"))
    // Versions opens over the player, never as a pushed page that would close it.
    #expect(screen.contains("chooseVersion: model.versions == nil ? nil : { openVersions() }"))
    #expect(try body(of: "private func openVersions()", in: screen).contains("model.cover()"))
    #expect(try body(of: "private func closeVersions()", in: screen).contains("model.uncover()"))
    #expect(!screen.contains("navigationDestination(isPresented: $showVersions)"))
    #expect(try body(of: "private func another(", in: screen).contains("PlayerLayout.offersAnother(model.playback, status: status)"))
}
