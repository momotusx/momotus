import Foundation
import Testing

@Test func recordsGoBackToTheCoreWithExactNumbers() throws {
    // The keystore record leaves the app through JSONSerialization; a 64-bit
    // id must not turn into a rounded double on the way.
    let record = try JSONDecoder().decode(JSONValue.self, from: Data(#"{"device":18446744073709551615,"epoch":3,"ok":true,"keys":["a",null]}"#.utf8))
    let text = String(decoding: try JSONSerialization.data(withJSONObject: ["record": record.foundation]), as: UTF8.self)
    #expect(text.contains("18446744073709551615"))
    #expect(text.contains(#""ok":true"#))
    let back = try JSONDecoder().decode([String: JSONValue].self, from: Data(text.utf8))
    #expect(back["record"] == record)
}

@Test func everySyncCodeHasPlainWords() {
    for code in ["dropped", "fork", "wrongRelay", "relayBehind", "notJoined", "unsent", "insecure", "kitUnknown"] {
        let words = SyncProblem(code: code, message: "raw").words
        #expect(words != "raw", "no words for \(code)")
        #expect(!words.contains(code), "\(code) leaks into the words")
    }
    #expect(SyncProblem(code: "kitUnknown", message: "raw", action: .recover).words == SyncWords.wrongKit)
    // Anything else may name the server's address: it reads as no answer.
    #expect(SyncProblem(code: nil, message: "the relay said no").words.hasPrefix("The sync server did not answer"))
    // A transport error never shows its raw text and URL.
    let offline = SyncProblem(code: nil, message: "network: error sending request (http://127.0.0.1:8787/v1/relay)").words
    #expect(!offline.contains("http"))
    #expect(offline.hasPrefix("The sync server did not answer"))
}

/// A report with every count at zero, then `changes` on top.
private func report(_ changes: [String: Any] = [:]) throws -> SyncReport {
    var fields: [String: Any] = [
        "pushed": 0, "stale": 0, "refused": 0, "waiting": 0, "pulled": 0, "applied": 0, "unreadable": 0,
        "newer": 0, "parked": 0, "oldEpochWrites": 0, "clockGapMs": 0, "rotationWaitMs": 0,
        "blocked": NSNull(), "fingerprint": "ABCDEFGHIJKL", "record": NSNull(), "again": false,
    ]
    fields.merge(changes) { $1 }
    return try JSONDecoder().decode(SyncReport.self, from: JSONSerialization.data(withJSONObject: fields))
}

@Test func reportNotesFollowTheProtocolThresholds() throws {
    #expect(try report().notes.isEmpty)
    // 10 minutes is the line: at it nothing is said, past it the clocks disagree.
    #expect(try report(["clockGapMs": 600_000]).notes.isEmpty)
    #expect(try report(["clockGapMs": -660_000]).notes.first?.contains("11 minutes") == true)
    let busy = try report(["waiting": 2, "parked": 1, "oldEpochWrites": 1, "blocked": "relayDisagrees"])
    #expect(busy.notes.count == 4)
    #expect(busy.notes.contains { $0.contains("12 characters") })
}
