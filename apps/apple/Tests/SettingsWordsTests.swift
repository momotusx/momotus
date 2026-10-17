import Foundation
import Testing

// Pairing, debrid and Home rows rules from the Android r5 review, as Apple has them.

@Test func pairingFailuresNeverShowConnectionText() {
    #expect(PairingWords.problem("pairing: the reply names another sync server") == "The reply names another sync server.")
    let silent = "The sync server did not answer. Check the address and try again."
    #expect(PairingWords.problem("network: error sending request for url (https://relay.example.org/v1/pair)") == silent)
    #expect(PairingWords.problem("http status 502 Bad Gateway") == silent)
    #expect(PairingWords.problem("invalid url: https://user:secret@relay.example.org") == "That is not a sync server address. It looks like https://sync.example.org.")
    #expect(PairingWords.problem("blocked private address 10.0.0.4") == "This device does not connect to that address.")
    #expect(PairingWords.problem("the relay must use https unless it is on this network") == "The sync server address must start with https.")
}

@Test func anAddressWithAUserNameOrPasswordIsRefused() {
    #expect(PairingWords.plainAddress("https://sync.example.org"))
    #expect(PairingWords.plainAddress("http://127.0.0.1:8787"))
    #expect(!PairingWords.plainAddress("https://me:secret@sync.example.org"))
    #expect(!PairingWords.plainAddress("https://me@sync.example.org"))
    #expect(!PairingWords.plainAddress("sync.example.org"))
}

@Test func debridSaysStoringAndHandingOverApart() {
    #expect(DebridWords.outcome(removed: false, inUse: 1) == "Saved on this device. 1 debrid service in use.")
    #expect(DebridWords.outcome(removed: true, inUse: 0) == "Removed from this device. No debrid service in use.")
    #expect(DebridWords.outcome(removed: false, inUse: nil, refused: "the core stopped") == "Saved on this device. The core did not take it: the core stopped.")
    // A Keychain refusal never says saved.
    #expect(!DebridWords.refused(removing: false).contains("Saved"))
    #expect(DebridWords.refused(removing: true) == "This device could not remove the key, so nothing changed.")
}

@Test func aHomeRowToggleReadsTheLastReply() {
    let shown = HomeCatalog(rowId: "a", title: "Popular", source: "Cinemeta", hidden: false)
    let hidden = HomeCatalog(rowId: "a", title: "Popular", source: "Cinemeta", hidden: true)
    // The first press hides it; the second, after the reply, shows it again.
    #expect(HomeCatalog.toggled("a", in: [shown]) == true)
    #expect(HomeCatalog.toggled("a", in: [hidden]) == false)
    #expect(HomeCatalog.toggled("b", in: [shown]) == nil)
    #expect(HomeCatalog.toggled("a", in: nil) == nil)
}

@Test func syncNotesReadInTheSingularForOne() throws {
    var fields: [String: Any] = [
        "pushed": 0, "stale": 0, "refused": 0, "waiting": 1, "pulled": 0, "applied": 0, "unreadable": 0,
        "newer": 0, "parked": 1, "oldEpochWrites": 0, "clockGapMs": 0, "rotationWaitMs": 60_000,
        "blocked": NSNull(), "fingerprint": "ABCDEFGHIJKL", "record": NSNull(), "again": false,
    ]
    let one = try JSONDecoder().decode(SyncReport.self, from: JSONSerialization.data(withJSONObject: fields)).notes
    #expect(one.contains("1 edit waits for the sync server's clock to catch up."))
    #expect(one.contains("1 record waits for a key this device does not have yet."))
    #expect(one.contains("Changes wait 1 minute after a sync server restore."))
    fields["waiting"] = 2
    let two = try JSONDecoder().decode(SyncReport.self, from: JSONSerialization.data(withJSONObject: fields)).notes
    #expect(two.contains("2 edits wait for the sync server's clock to catch up."))
}
