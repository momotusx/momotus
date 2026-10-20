import Foundation
import Testing

// Smart stream choice (plans/m13-stream-choice.md 3.7, 3.9, 3.11, 4.4, 4.5):
// the words from apps/shared/stream-choice-words.json, the rules its "about"
// gives, the new fields' decoding, and the ops on a real core.

private func line(_ json: String) throws -> ReasonLine {
    try JSONDecoder().decode(ReasonLine.self, from: Data(json.utf8))
}

private func words(_ json: String) throws -> LineWords {
    try #require(StreamChoice.lineWords(try line(json)))
}

private func learned(size: Int = 0, height: Int = 0, ready: Int = 0, hdr: Int = 0, epoch: Int = 0, muted: String = "[]") -> String {
    #"{"ready": \#(ready), "height": \#(height), "hdr": \#(hdr), "size": \#(size), "picks": 4, "epoch": \#(epoch), "atMs": 1, "muted": \#(muted)}"#
}

private let streams = #"{"quality": "balanced", "maxHeight": null, "maxSizeBytes": null, "languages": [], "avoid": [], "sizeLimitBytes": 5368709120, "minSeeders": 50, "hdr": true, "preferReady": true, "learn": true}"#

private func prefs(uhd: String = learned(), fhd: String = learned(), epoch: Int = 0, streams: String = streams) throws -> Prefs {
    let json = #"{"streams": \#(streams), "subtitleLanguages": [], "autoplayNext": true, "defaultsSeen": 1, "#
        + #""learned": {"uhd": \#(uhd), "fhd": \#(fhd), "hd": \#(learned()), "sd": \#(learned())}, "learnedEpoch": \#(epoch)}"#
    return try JSONDecoder().decode(Prefs.self, from: Data(json.utf8))
}

@Test func theSharedWordsLoadWithEveryLine() {
    let kinds = ["copy", "fetch", "stalled", "noPicture", "didNotPlay", "veryFew", "otherYear", "mayPause", "washedOut", "large", "few", "ready", "quick", "should"]
    #expect(Set(StreamChoice.words.lines.keys) == Set(kinds))
    #expect(StreamChoice.words.question.fast.patch.sizeLimitBytes == 5_368_709_120)
}

@Test func sizesUseTheAddonsUnits() {
    #expect(StreamChoice.bytes(5_368_709_120) == "5 GB")
    #expect(StreamChoice.bytes(1_181_116_006) == "1.1 GB")
    #expect(StreamChoice.bytes(164_626_432) == "157 MB")
    #expect(StreamChoice.bytes(150 * 1_073_741_824) == "150 GB")
    #expect(StreamChoice.bytes(10_737_418_240) == "10 GB")
    #expect(StreamChoice.bytes(900) == "1 KB")
    #expect(StreamChoice.counted("seeders", 1) == "1 seeder")
    #expect(StreamChoice.counted("seeders", 1204) == "1,204 seeders")
}

@Test func eachLineSaysItsToneTextAndDetails() throws {
    #expect(try words(#"{"kind": "veryFew", "seeders": 3}"#) == LineWords(tone: .bad, text: "May not start", details: "3 seeders"))
    #expect(try words(#"{"kind": "few", "seeders": 7}"#) == LineWords(tone: .warn, text: "May start slowly", details: "7 seeders"))
    #expect(try words(#"{"kind": "copy", "quality": "CAM"}"#).details == "CAM copy")
    #expect(try words(#"{"kind": "otherYear", "year": 1999}"#).details == "The release says 1999")
    // A part whose field is unknown is left out, and the rest starts with a capital.
    #expect(try words(#"{"kind": "should", "seeders": null, "sizeBytes": 1181116006, "service": "Real-Debrid"}"#).details
        == "1.1 GB, may be ready on Real-Debrid")
    #expect(try words(#"{"kind": "fetch", "service": null}"#).details == "")
    #expect(try words(#"{"kind": "ready", "service": "TorBox", "sizeBytes": null}"#) == LineWords(tone: .ok, text: "Starts right away", details: "Ready on TorBox"))
    #expect(try words(#"{"kind": "quick", "seeders": 120, "sizeBytes": 164626432}"#).details == "120 seeders, 157 MB")
    // Measured from playbacks, or an estimate.
    #expect(try words(#"{"kind": "mayPause", "needMbps": 31.6, "haveMbps": 9.4, "samples": 3}"#).details
        == "Needs about 32 Mbit/s; this connection gives about 9 (measured from 3 playbacks)")
    #expect(try words(#"{"kind": "mayPause", "needMbps": 31.6, "haveMbps": 10, "samples": 0}"#).details
        == "Needs about 32 Mbit/s; this connection gives about 10 (estimated)")
    // Large: the default words, or the chosen limit's.
    let large = try line(#"{"kind": "large", "sizeBytes": 9663676416, "limitBytes": 5368709120}"#)
    #expect(StreamChoice.lineWords(large)?.text == "Large file")
    #expect(StreamChoice.lineWords(large, limitChosen: true)?.text == "Larger than the size you chose")
    #expect(StreamChoice.lineWords(large)?.details == "9 GB; Marquee prefers files up to 5 GB")
    // A newer core's kind has no words here: the row keeps its status.
    #expect(StreamChoice.lineWords(try line(#"{"kind": "somethingNew"}"#)) == nil)
}

@Test func blockedVersionsSayWhyAndOfferTheirAction() throws {
    let blocked = try JSONDecoder().decode(BlockedVersions.self, from: Data(#"{"reason": "needsAccount", "count": 12, "rows": []}"#.utf8))
    let w = try #require(StreamChoice.blockedWords(blocked))
    #expect(w.text.hasPrefix("Marquee found 12 versions. This version of the app plays them only through a streaming service account"))
    #expect(w.action == "Add an account")
    let none = try JSONDecoder().decode(BlockedVersions.self, from: Data(#"{"reason": "nothingPlaysHere", "count": 1, "rows": []}"#.utf8))
    #expect(StreamChoice.blockedWords(none)?.text == "Found 1 version, but none plays on this device.")
}

@Test func theDeviceLineNamesTheScreenTheConnectionAndTheDisk() {
    let measured = Speed(mbps: 41.6, samples: 2), estimate = Speed(mbps: 15, samples: 0)
    #expect(StreamChoice.conditionsLine(screenClass: 2160, hdr: true, network: "wifi", speed: measured, freeBytes: 65_498_251_264)
        == "4K HDR, about 42 Mbit/s on Wi-Fi, 61 GB free")
    #expect(StreamChoice.conditionsLine(screenClass: 1080, hdr: false, network: "other", speed: estimate, freeBytes: nil)
        == "1080p, this network, speed not measured yet")
    // With an account, or without the engine, HTTP decides; else the torrent engine.
    #expect(StreamChoice.speed(http: measured, p2p: estimate, engine: true, account: false) == estimate)
    #expect(StreamChoice.speed(http: measured, p2p: estimate, engine: true, account: true) == measured)
    #expect(StreamChoice.speed(http: measured, p2p: estimate, engine: false, account: false) == measured)
    let v = VersionsConditions(
        screenClass: 1080, hdr: false, network: "ethernet", http: measured, p2p: estimate, freeBytes: nil, runtimeMin: 50,
        sizeLimitBytes: 5_368_709_120, minSeeders: 50, engine: false, account: false)
    #expect(StreamChoice.conditionsHeader(device: "desktop", v) == "This computer: 1080p, about 42 Mbit/s on Ethernet")
    #expect(StreamChoice.network(wifi: true, ethernet: false, cellular: false) == "wifi")
    #expect(StreamChoice.network(wifi: true, ethernet: true, cellular: false) == "ethernet")
    #expect(StreamChoice.network(wifi: false, ethernet: false, cellular: true) == "cellular")
    #expect(StreamChoice.network(wifi: false, ethernet: false, cellular: false) == "other")
}

@Test func versionsGetTheScreensHDRAndTheTitlesFacts() {
    #expect(StreamChoice.hdrNames(eligible: false, hdr10: true, hlg: true, dolbyVision: true).isEmpty)
    #expect(StreamChoice.hdrNames(eligible: true, hdr10: true, hlg: true, dolbyVision: false) == ["hdr10", "hlg"])
    #expect(StreamChoice.hdrNames(eligible: true, hdr10: true, hlg: true, dolbyVision: true) == ["hdr10", "hlg", "dolby_vision"])
    let facts = StreamChoice.versionsFacts(runtime: "142 min", releaseInfo: "2010", released: "2010-09-30", episodesInSeason: 10)
    #expect(facts["runtime"] as? String == "142 min" && facts["releaseInfo"] as? String == "2010")
    #expect(facts["released"] as? String == "2010-09-30" && facts["episodesInSeason"] as? Int == 10)
    // Left out when unknown: the core reads the meta instead.
    #expect(StreamChoice.versionsFacts(runtime: nil, releaseInfo: nil, released: nil, episodesInSeason: 0).isEmpty)
}

@Test func learnedNotesNameTheirNumbersAndSettingsListsWhatIsLearned() throws {
    let note = try JSONDecoder().decode(LearnedNote.self, from: Data(#"{"screenClass": 2160, "choice": "size", "up": true, "prefs": \#(streams.replacingOccurrences(of: "5368709120", with: "10737418240"))}"#.utf8))
    #expect(StreamChoice.learnedNote(note, device: "tv") == "On this TV you often picked bigger files, so Marquee now picks files up to 10 GB.")
    // A change that names no number (the screen's resolution) has no note.
    let height = try JSONDecoder().decode(LearnedNote.self, from: Data(#"{"screenClass": 1080, "choice": "height", "up": true, "prefs": \#(streams)}"#.utf8))
    #expect(StreamChoice.learnedNote(height, device: "desktop") == nil)

    let p = try prefs(uhd: learned(size: 3, hdr: 2, epoch: 1), fhd: learned(height: -4, ready: 5, epoch: 1, muted: #"["ready"]"#), epoch: 1)
    #expect(StreamChoice.learnedRows(p) == [
        LearnedRow(screenClass: 2160, choice: "size", up: true, text: "On 4K screens you often picked bigger files."),
        LearnedRow(screenClass: 1080, choice: "height", up: false, text: "On 1080p screens you often picked lower resolutions."),
    ])
    #expect(StreamChoice.anyLearned(p))
    // Forgotten: every counter stamped below the epoch reads as zero.
    let forgotten = try prefs(uhd: learned(size: 3, epoch: 1), epoch: 2)
    #expect(StreamChoice.learnedRows(forgotten).isEmpty)
    #expect(!StreamChoice.anyLearned(forgotten))
}

@Test func theSettingsRowSaysTheSizeAndThePeople() throws {
    #expect(StreamChoice.summary(try prefs().streams) == "Up to 5 GB, shared by at least 50 people")
    let custom = try prefs(streams: streams.replacingOccurrences(of: "5368709120", with: "3221225472").replacingOccurrences(of: "\"minSeeders\": 50", with: "\"minSeeders\": 1500"))
    #expect(StreamChoice.summary(custom.streams) == "Up to 3 GB, shared by at least 1,500 people")
    let any = try prefs(streams: streams.replacingOccurrences(of: "5368709120", with: "null"))
    #expect(StreamChoice.summary(any.streams) == "Any size, shared by at least 50 people")
}

@Test func theQuestionsAnswersArePatchesOfTheStreamPreferences() throws {
    let fast = try #require(StreamChoice.patch(StreamChoice.words.question.fast)["streams"] as? [String: any Sendable])
    #expect(fast["sizeLimitBytes"] as? Int64 == 5_368_709_120)
    #expect(fast["minSeeders"] as? Int == 50)
    let best = try #require(StreamChoice.patch(StreamChoice.words.question.best)["streams"] as? [String: any Sendable])
    #expect(best["sizeLimitBytes"] is NSNull)
    #expect(best["minSeeders"] as? Int == 20)
    #expect(StreamChoice.words.question.skip.means == "fast")
}

@Test func theNewVersionsFieldsDecodeAndOlderCoresStillDo() throws {
    let rowJSON = #"{"key": "k", "quality": "1080p", "features": [], "sizeBytes": 1, "status": {"kind": "torrent", "seeds": 7}, "via": "Torrentio", "releaseName": "A.mp4", "languages": [], "reasons": [], "group": "best", "failoverCount": 0"#
    let row = try JSONDecoder().decode(VersionRow.self, from: Data((rowJSON + #", "line": {"kind": "few", "seeders": 7}, "sourceClass": "swarm", "bitrateMbps": 4.2, "score": {}}"#).utf8))
    #expect(row.line?.kind == "few" && row.line?.seeders == 7 && row.sourceClass == "swarm")
    #expect(try JSONDecoder().decode(VersionRow.self, from: Data((rowJSON + "}").utf8)).line == nil)
    let snap = #"{"recommended": null, "rows": [], "hidden": {"count": 0, "byReason": []}, "pendingAddons": [], "slowAddons": [], "failedAddons": [], "done": true, "frozen": false"#
    let full = try JSONDecoder().decode(VersionsSnapshot.self, from: Data((snap + #", "conditions": {"screenClass": 2160, "hdr": true, "network": "wifi", "http": {"mbps": 15, "samples": 0}, "p2p": {"mbps": 10, "samples": 0}, "freeBytes": 1024, "runtimeMin": 22, "sizeLimitBytes": null, "minSeeders": 50, "engine": true, "account": false}, "blocked": {"reason": "noSpace", "count": 2, "rows": []}, "note": null}"#).utf8))
    #expect(full.conditions?.screenClass == 2160 && full.blocked?.reason == "noSpace" && full.note == nil)
    #expect(try JSONDecoder().decode(VersionsSnapshot.self, from: Data((snap + "}").utf8)).conditions == nil)
}

@Test func almostNobodySharingDecodesWithItsSharedWords() throws {
    let snap = #"{"recommended": null, "rows": [], "hidden": {"count": 0, "byReason": []}, "pendingAddons": [], "slowAddons": [], "failedAddons": [], "done": true, "frozen": false"#
    #expect(try JSONDecoder().decode(VersionsSnapshot.self, from: Data((snap + #", "nobodySharing": true}"#).utf8)).nobodySharing == true)
    #expect(try JSONDecoder().decode(VersionsSnapshot.self, from: Data((snap + #", "nobodySharing": false}"#).utf8)).nobodySharing == false)
    // An older core sends none: nothing is said.
    #expect(try JSONDecoder().decode(VersionsSnapshot.self, from: Data((snap + "}").utf8)).nobodySharing == nil)
    // The words every app shows, from apps/shared/stream-choice-words.json.
    #expect(StreamChoice.words.nobodySharing.text == "Almost nobody is sharing this right now")
    #expect(StreamChoice.words.nobodySharing.details == "Every version has one seeder or none at the moment, so it may not start.")
}

@Suite(.serialized) @MainActor struct StreamChoiceWiringTests {
    @Test func settingsReadsTheDeviceAndMeasureAgainAnswersIt() async throws {
        let core = RealCore()
        #expect(await core.ready())
        let c = try await core.client.call("streamConditions", as: StreamConditions.self)
        #expect(["wifi", "ethernet", "cellular", "other"].contains(c.network))
        #expect(!StreamChoice.measured(c))
        let reset = try await core.client.call("resetSpeed", as: StreamConditions.self)
        #expect(reset.http.samples == 0 && reset.p2p.samples == 0)
    }

    @Test func forgetAndUndoAnswerWithTheFullPreferences() async throws {
        let core = RealCore()
        #expect(await core.ready())
        let before = try await core.client.call("forgetLearned", as: Prefs.self)
        let again = try await core.client.call("forgetLearned", as: Prefs.self)
        #expect(again.learnedEpoch > before.learnedEpoch)
        let undone = try await core.client.call("undoLearned", ["screenClass": 1080, "choice": "size"], as: Prefs.self)
        #expect(undone.learned.fhd.size == 0)
        #expect(undone.learned.fhd.muted.contains("size"))
        // The question's answer is a patch the core takes.
        try await core.client.perform("setPrefs", ["patch": StreamChoice.patch(StreamChoice.words.question.best)])
        let p = try await core.client.call("forgetLearned", as: Prefs.self)
        #expect(p.streams.sizeLimitBytes == nil && p.streams.minSeeders == 20)
    }
}

@Test func aRowReadsItsLineToAScreenReader() throws {
    let base = #"{"key": "k", "quality": "720p", "features": [], "sizeBytes": 1, "status": {"kind": "torrent", "seeds": 7}, "via": "Torrentio", "releaseName": "A.mp4", "languages": [], "reasons": [], "group": "best", "failoverCount": 0"#
    let row = try JSONDecoder().decode(VersionRow.self, from: Data((base + #", "line": {"kind": "few", "seeders": 7}}"#).utf8))
    #expect(StreamChoice.spoken(row) == "720p, May start slowly, 7 seeders, via Torrentio")
    // An older core's row says its status.
    let old = try JSONDecoder().decode(VersionRow.self, from: Data((base + "}").utf8))
    #expect(StreamChoice.spoken(old) == "720p, Torrent, 7 seeds, via Torrentio")
}

@Test func theResolutionChoiceAndTheScreenClassReadAsTheWebsDo() {
    #expect(StreamChoice.screenClass(lines: 2160) == 2160)
    #expect(StreamChoice.screenClass(lines: 1179) == 1080)
    #expect(StreamChoice.screenClass(lines: 720) == 720)
    #expect(StreamChoice.screenClass(lines: 640) == 480)
    // "This screen's" for no cap and for a 4K cap; else the cap itself.
    #expect(StreamChoice.resolutionPicked(nil, maxHeight: nil))
    #expect(StreamChoice.resolutionPicked(nil, maxHeight: 2160))
    #expect(!StreamChoice.resolutionPicked(nil, maxHeight: 1080))
    #expect(StreamChoice.resolutionPicked(1080, maxHeight: 1080))
    #expect(!StreamChoice.resolutionPicked(720, maxHeight: 1080))
}

@Test func versionsPinsItsPickAgainAfterTheCoreLetsOneGo() {
    // Open, and the core has not pinned yet (or let a failed pick go): pin.
    #expect(StreamChoice.shouldFreeze(open: true, frozen: nil))
    #expect(StreamChoice.shouldFreeze(open: true, frozen: false))
    // Already pinned, or not open: nothing to send.
    #expect(!StreamChoice.shouldFreeze(open: true, frozen: true))
    #expect(!StreamChoice.shouldFreeze(open: false, frozen: false))
}

@Test func almostNobodySharingShowsOncePerScreen() {
    // On the recommended card right before Play when there is one, never also over the list.
    #expect(StreamChoice.nobodySharingPlace(true, hasRecommended: true) == .card)
    // No card (every version blocked or hidden): over the list.
    #expect(StreamChoice.nobodySharingPlace(true, hasRecommended: false) == .list)
    #expect(StreamChoice.nobodySharingPlace(false, hasRecommended: true) == nil)
    #expect(StreamChoice.nobodySharingPlace(nil, hasRecommended: false) == nil)
}
