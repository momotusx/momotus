import Foundation
import Testing

// Sync rules and words (the web's lib/sync.ts and words.ts, Apple's own
// failure words per action), and answers recorded from real cores on a
// throwaway relay: a desk that started the account and made a kit, a tablet
// it added, a phone and a kit the tablet added (records stripped).

private typealias W = SyncWords

private func fixture<T: Decodable>(_ name: String, as type: T.Type) throws -> T {
    let url = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "Fixtures/\(name)")
    return try JSONDecoder().decode(T.self, from: Data(contentsOf: url))
}

private func member(_ id: String, name: String? = nil, kit: Bool = false, sponsor: String? = nil, addedMs: Int64 = 0, this: Bool = false) -> MemberInfo {
    MemberInfo(id: id, kit: kit, name: name, sponsor: sponsor, addedMs: addedMs, thisDevice: this)
}

private func report(_ changes: [String: Any] = [:]) throws -> SyncReport {
    var fields: [String: Any] = [
        "pushed": 0, "stale": 0, "refused": 0, "waiting": 0, "pulled": 0, "applied": 0, "unreadable": 0,
        "newer": 0, "parked": 0, "oldEpochWrites": 0, "clockGapMs": 0, "rotationWaitMs": 0,
        "blocked": NSNull(), "fingerprint": "ABCDEFGHIJKL", "record": NSNull(), "again": false,
    ]
    fields.merge(changes) { $1 }
    return try JSONDecoder().decode(SyncReport.self, from: JSONSerialization.data(withJSONObject: fields))
}

@Suite struct SyncRulesTests {
    @Test func recordedAnswersDecode() throws {
        let devices = try fixture("sync-devices.json", as: SyncDevices.self)
        #expect(devices.members.count == 5)
        #expect(devices.fingerprint.count == 12)
        #expect(devices.record == nil)
        let sorted = SyncRules.sortMembers(devices.members).map(SyncRules.memberName)
        #expect(sorted == ["Desk", "Tablet", "Phone", "Recovery kit", "Recovery kit"])
        let preview = try fixture("sync-lost-preview.json", as: LostPreview.self)
        let tablet = try #require(devices.members.first { $0.name == "Tablet" })
        // The tablet added the phone and one kit: the preview lists both, by id.
        #expect(preview.devices == devices.members.filter { $0.sponsor == tablet.id && !$0.kit }.map(\.id))
        #expect(preview.kits == devices.members.filter { $0.sponsor == tablet.id && $0.kit }.map(\.id))
        let round = try fixture("sync-report.json", as: SyncReport.self)
        #expect(round.fingerprint.count == 12)
        #expect(round.record == nil && !round.again)
    }

    @Test func aLostReportListsWhatItsDeviceAdded() throws {
        let devices = try fixture("sync-devices.json", as: SyncDevices.self)
        let preview = try fixture("sync-lost-preview.json", as: LostPreview.self)
        let tablet = try #require(devices.members.first { $0.name == "Tablet" })
        let rows = SyncRules.lostRows(preview: preview, members: devices.members)
        #expect(rows.map(\.name) == ["Phone"])
        #expect(rows[0].detail.hasPrefix("Added ") && rows[0].detail.hasSuffix(" through Tablet"))
        // Ticked: the tablet and the phone go, and the tablet's kit with them.
        let drop = SyncRules.lostDrop(lost: tablet.id, added: preview.devices, keep: [])
        #expect(drop.count == 2)
        #expect(SyncRules.kitsGoing(previewKits: preview.kits, members: devices.members, keep: []).going == preview.kits)
        #expect(W.lostButton(drop.count) == "Remove 2 devices")
        // The phone unticked: only the tablet goes; its kit still does.
        let kept = SyncRules.lostDrop(lost: tablet.id, added: preview.devices, keep: Set(preview.devices))
        #expect(kept == [tablet.id])
        #expect(SyncRules.kitsGoing(previewKits: preview.kits, members: devices.members, keep: Set(preview.devices)).going == preview.kits)
    }

    @Test func aDeviceTheListDoesNotHaveYetIsStillShown() {
        // The preview names a device added after the list was read: it is listed, never dropped unseen.
        let members = [member("a", name: "Desk", this: true), member("b", name: "Tablet", sponsor: "a", addedMs: 1_700_000_000_000)]
        let rows = SyncRules.lostRows(preview: LostPreview(devices: ["b", "zz"], kits: []), members: members)
        #expect(rows.map(\.id) == ["b", "zz"])
        #expect(rows[1].name == "Unknown device")
        #expect(SyncRules.lostDrop(lost: "x", added: ["b", "zz"], keep: ["b"]) == ["x", "zz"])
    }

    @Test func kitsGoUnlessTheirMakerStays() {
        // The lost device "l" added "t"; "l" made k1 and "t" made k2.
        let members = [member("a", this: true), member("l"), member("t", sponsor: "l"), member("k1", kit: true, sponsor: "l"), member("k2", kit: true, sponsor: "t")]
        // A kit whose maker goes goes too; one whose maker the user unticked stays.
        #expect(SyncRules.kitsGoing(previewKits: ["k1", "k2"], members: members, keep: []).going == ["k1", "k2"])
        #expect(SyncRules.kitsGoing(previewKits: ["k1", "k2"], members: members, keep: ["t"]) == (["k1"], []))
        // A kit the list does not have yet: going while nothing is kept, else nobody can say.
        #expect(SyncRules.kitsGoing(previewKits: ["k9"], members: members, keep: []) == (["k9"], []))
        #expect(SyncRules.kitsGoing(previewKits: ["k9"], members: members, keep: ["t"]) == ([], ["k9"]))
        #expect(W.lostKitsUnsure(1) == "1 recovery kit this device does not list yet may stop working too, if the device that made it goes. Check the list afterwards.")
        #expect(W.lostKits(1) == "1 recovery kit made on these devices stops working. Make a new one afterwards.")
        #expect(W.lostKits(2).hasPrefix("2 recovery kits made on these devices stop working."))
    }

    @Test func membersReadInOrderWithTheirNames() {
        let members = [
            member("k", kit: true, sponsor: "a", addedMs: 5), member("old", name: "Old", sponsor: "a", addedMs: 2),
            member("a", name: "Desk", addedMs: 9, this: true), member("e", name: "", sponsor: "gone", addedMs: 3),
        ]
        #expect(SyncRules.sortMembers(members).map(\.id) == ["a", "old", "e", "k"])
        #expect(SyncRules.memberName(members[0]) == "Recovery kit")
        #expect(SyncRules.memberName(members[3]) == "Unnamed device")
        #expect(SyncRules.sponsorName(members[3], in: members) == "a removed device")
        #expect(SyncRules.sponsorName(members[2], in: members) == nil)
        #expect(SyncRules.sponsorName(members[1], in: members) == "Desk")
    }

    @Test func addedLinesSayWhenAndThrough() {
        let noon = Int64(1_760_875_200_000) // 2025-10-19 12:00 UTC: the same day in every time zone near it
        #expect(W.addedLine(kit: false, addedMs: noon, by: nil).hasSuffix(", the first device"))
        #expect(W.addedLine(kit: false, addedMs: noon, by: "Desk").hasSuffix(" through Desk"))
        #expect(W.addedLine(kit: true, addedMs: noon, by: "Desk").hasPrefix("Made Oct"))
        #expect(W.addedLine(kit: true, addedMs: noon, by: "Desk").hasSuffix(" on Desk"))
    }

    @Test func onlyAPairingCodeIsTakenAndTheQuestionIsTheDevice() {
        let code = "marquee:pair?v=2&p=slot&s=https%3A%2F%2Fsync.example.org&k=AAAA&g=BBBB"
        #expect(SyncRules.isPairingCode("  \(code)\n"))
        #expect(!SyncRules.isPairingCode("https://example.org/manifest.json"))
        #expect(!SyncRules.isPairingCode("marquee:pairing"))
        // The code is always for the user's own sync server: the question names no host.
        #expect(W.reviewAsk == "Approve a new device?")
        #expect(!W.reviewText.contains("sync.example.org"))
    }

    @Test func theTwelveCharactersReadInGroups() {
        #expect(SyncRules.fingerprintGroups("VW4BV9DJD5EZ") == "VW4B V9DJ D5EZ")
        #expect(SyncRules.fingerprintGroups("ABCDEF") == "ABCD EF")
        #expect(SyncRules.fingerprintGroups("") == "")
    }

    @Test func aRecordNeverRollsBack() throws {
        let decode = { (text: String) in try JSONDecoder().decode(JSONValue.self, from: Data(text.utf8)) }
        let five = try decode(#"{"revision":5,"server":"a"}"#), four = try decode(#"{"revision":4,"server":"b"}"#)
        #expect(SyncRules.newer(five, four) == five)
        #expect(SyncRules.newer(four, five) == five)
        #expect(SyncRules.newer(nil, four) == four)
        // Equal revisions: the answer wins (it is the later word on the same revision).
        let fiveAgain = try decode(#"{"revision":5,"server":"c"}"#)
        #expect(SyncRules.newer(five, fiveAgain) == fiveAgain)
        #expect(SyncRules.revision(try decode(#"{"server":"a"}"#)) == 0)
    }

    @Test func failuresAreWordedForTheirAction() {
        let words = { (message: String, action: SyncAction) in SyncProblem(code: nil, message: message, action: action).words }
        #expect(words("http status 403", .start) == "This sync server needs an invite code, or the one entered is wrong.")
        #expect(words("http status 404", .recover) == W.wrongKit)
        #expect(words("http status 404", .approve).hasPrefix("This code expired or was used already."))
        #expect(words("http status 410", .approve).hasPrefix("This code expired or was used already."))
        // A 404 while making a kit, removing or reporting is no expired code.
        for action in [SyncAction.kit, .remove, .lost, .devices, .leave] {
            #expect(!words("http status 404", action).contains("code expired"), "\(action)")
            #expect(words("http status 404", action).contains("The sync server did not answer"), "\(action)")
        }
        #expect(words("http status 403", .recover) != words("http status 403", .start))
        // The core's own sentences are kept, an address never shows.
        #expect(words("roster: sign out to remove this device", .remove) == "Sign out to remove this device.")
        #expect(words("pairing: the code is for another sync server", .approve) == "The code is for another sync server.")
        #expect(!words("network: error sending request (http://127.0.0.1:8787/v1/relay)", .round).contains("http"))
        #expect(words("network: error", .round).hasPrefix("The sync server did not answer. Your changes wait here"))
        #expect(words("network: error", .devices).contains("out of date"))
    }

    @Test func everyCodeHasPlainWords() {
        for code in ["dropped", "fork", "wrongRelay", "relayBehind", "notJoined", "unsent", "insecure", "storage", "unconfirmed", "noPasscode", "rosterChanged", "wrongRecord"] {
            let words = SyncProblem(code: code, message: "raw").words
            // No protocol name in the words ("storage" is a plain word there).
            #expect(words != "raw" && (code == "storage" || !words.contains(code)), "\(code)")
        }
        // An error with a code wins over the action's own reading.
        #expect(SyncProblem(code: "dropped", message: "http status 404", action: .approve).words.hasPrefix("This device was removed"))
        #expect(SyncProblem(CoreError("x", code: "unsent"), action: .leave).code == "unsent")
    }

    @Test func everyReportFactIsSaid() throws {
        #expect(try report().notes.isEmpty)
        // 10 minutes is the line either way: at it nothing is said, past it the clocks disagree.
        #expect(try report(["clockGapMs": 600_000]).notes.isEmpty)
        #expect(try report(["clockGapMs": -660_000]).notes.first?.contains("11 minutes") == true)
        #expect(try report(["clockGapMs": 660_000]).notes.first?.contains("11 minutes") == true)
        let facts: [(String, Any, String)] = [
            ("waiting", 2, "2 edits wait for the sync server's clock"),
            ("parked", 1, "1 change from another device waits"),
            ("oldEpochWrites", 1, "may be hiding a device change"),
            ("rotationWaitMs", 90_000, "Edits wait 2 minutes more"),
            ("refused", 1, "1 edit was too large or malformed"),
            ("refused", 3, "they stay only on this device"),
            ("unreadable", 2, "2 changes from another device did not open"),
            ("newer", 1, "from a newer version of Marquee"),
            ("blocked", "rotationPending", "until the account finishes a device change"),
            ("blocked", "notActive", "does not take edits from this device yet"),
            ("blocked", "relayDisagrees", "disagrees about your device list"),
        ]
        for (field, value, words) in facts {
            let notes = try report([field: value]).notes
            #expect(notes.count == 1 && notes[0].contains(words), "\(field) \(value): \(notes)")
        }
    }

    @Test func linesFillTheirNames() {
        #expect(W.named(W.lostLede, "Tablet").hasPrefix("Tablet is removed from your account"))
        #expect(W.removeDeviceAsk("Tablet").hasPrefix("Remove Tablet? It stops syncing"))
        #expect(W.count(1, "device", "devices") == "1 device")
        #expect(W.lostButton(1) == "Remove 1 device")
    }

    @Test func failuresStayPastACleanRoundUntilDismissed() throws {
        var failed = SyncFailed()
        failed = failed.adding(try report(["refused": 2, "unreadable": 1]))
        failed = failed.adding(try report())
        #expect(failed == SyncFailed(refused: 2, unreadable: 1, newer: 0))
        failed = failed.adding(try report(["newer": 1]))
        #expect(W.failedSince(failed) == "Since this device joined, 2 edits from this device could not be sent and stay here only, and 1 change from other devices could not be read, and 1 change from a newer version of Marquee waits for an update here.")
        #expect(W.failedSince(SyncFailed()) == nil)
    }

    @Test func settingsNeverSaysSyncedOverAWarning() throws {
        let at = Date(timeIntervalSince1970: 1_700_000_000)
        let notes = try report(["waiting": 1]).notes
        #expect(W.summary(joined: true, problem: nil, failed: nil, notes: [], at: at).hasPrefix("Synced "))
        #expect(W.summary(joined: true, problem: nil, failed: nil, notes: notes, at: at) == notes[0])
        #expect(W.summary(joined: true, problem: nil, failed: "Since this device joined, 1 edit", notes: notes, at: at) == "Since this device joined, 1 edit")
        #expect(W.summary(joined: true, problem: "The sync server did not answer.", failed: "x", notes: notes, at: at) == "The sync server did not answer.")
        #expect(W.summary(joined: false, problem: nil, failed: nil, notes: [], at: nil) == W.offLine)
        #expect(W.summary(joined: true, problem: nil, failed: nil, notes: [], at: nil) == W.onLine)
    }

    @Test func aNewWarningIsSaidOnceAndARepeatSaysNothing() {
        let stale = SyncRules.warnings(problem: nil, failed: nil, notes: ["1 edit waits."], listStale: true)
        #expect(stale == ["1 edit waits.", W.listStale])
        #expect(SyncRules.warnings(problem: "No answer.", failed: "Since", notes: [], listStale: false) == ["No answer.", "Since"])
        #expect(SyncRules.announcement(old: [], new: stale) == "1 edit waits. \(W.listStale)")
        // The next round says the same: nothing new is said.
        #expect(SyncRules.announcement(old: stale, new: stale) == nil)
        #expect(SyncRules.announcement(old: stale, new: ["1 edit waits."]) == nil)
        #expect(SyncRules.announcement(old: ["1 edit waits."], new: stale) == W.listStale)
    }

    @Test func removeNamesTheKitsThatGoAndTickRowsReadApart() {
        let members = [member("a", this: true), member("t", name: "Tablet", sponsor: "a"),
                       member("k1", kit: true, sponsor: "t", addedMs: 1_700_000_000_000), member("k2", kit: true, sponsor: "t", addedMs: 1_710_000_000_000)]
        #expect(SyncRules.kitsMade(by: "t", in: members) == 2)
        #expect(SyncRules.kitsMade(by: "a", in: members) == 0)
        #expect(W.removeDeviceAsk("Tablet", kits: 2).hasSuffix("Devices it added stay. The 2 recovery kits made on it stop working too."))
        #expect(W.removeDeviceAsk("Tablet", kits: 1).hasSuffix(" The recovery kit made on it stops working too."))
        #expect(W.removeDeviceAsk("Tablet").hasSuffix("Devices it added stay."))
        // Two "Recovery kit" rows: VoiceOver reads their lines too, so they differ.
        let spoken = members.filter(\.kit).map {
            SyncRules.spokenRow(SyncRules.memberName($0), W.addedLine(kit: true, addedMs: $0.addedMs, by: SyncRules.sponsorName($0, in: members)))
        }
        #expect(spoken[0] != spoken[1])
        #expect(spoken[0].hasPrefix("Recovery kit, Made "))
    }
}
