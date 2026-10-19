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

@Test func aDebridCardSaysConnectedOnlyOnceStored() {
    let ok = DebridAccount(service: "torbox", generation: "g", state: "ok")
    #expect(DebridWords.state(nil, pending: false) == nil)
    #expect(DebridWords.state(ok, pending: true) == "Saving")
    #expect(DebridWords.state(ok, pending: false) == "Connected")
    #expect(DebridWords.state(DebridAccount(service: "torbox", generation: "g", state: "reconnect"), pending: false) == "Sign in again: the service asked for it")
    #expect(DebridWords.state(DebridAccount(service: "torbox", generation: "g", state: "failing"), pending: true) == "Not answering, trying again")
    #expect(DebridWords.login(.used) == "Another sign-in or key took its place.")
    #expect(DebridWords.goodFor(299_001) == "Good for 5:00")
    #expect(DebridWords.address("https://www.real-debrid.com/device/") == "real-debrid.com/device")
    #expect(DebridWords.inUse(2) == "2 debrid services in use.")
}

@Test func aDebridCardOffersItsSignInOnlyWhileItNeedsOne() {
    #expect(DebridWords.offersSignIn("torbox", nil))
    #expect(DebridWords.offersSignIn("torbox", DebridAccount(service: "torbox", generation: "g", state: "reconnect")))
    // Connected, or failing for now: no sign-in, so no primary on the card.
    #expect(!DebridWords.offersSignIn("torbox", DebridAccount(service: "torbox", generation: "g", state: "ok")))
    #expect(!DebridWords.offersSignIn("torbox", DebridAccount(service: "torbox", generation: "g", state: "failing")))
    // Premiumize takes a key only.
    #expect(!DebridWords.offersSignIn("premiumize", nil))
}

@Test func aSignInByCodeEndsOnlyWithItsOwnStoredRecord() {
    // Done on the service's side, but the account still has the generation from before.
    #expect(!DebridWords.signedIn(done: true, before: "old", now: "old", pending: false))
    // The new generation is listed but its record is not stored yet.
    #expect(!DebridWords.signedIn(done: true, before: "old", now: "new", pending: true))
    #expect(DebridWords.signedIn(done: true, before: "old", now: "new", pending: false))
    #expect(DebridWords.signedIn(done: true, before: nil, now: "new", pending: false))
    #expect(!DebridWords.signedIn(done: true, before: nil, now: nil, pending: false))
    #expect(DebridWords.doneWords(stored: true) == "Saving")
    #expect(DebridWords.doneWords(stored: false) == "Kept for this session")
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
    #expect(one.contains("1 edit waits for the sync server's clock to catch up. Nothing is lost."))
    #expect(one.contains("1 change from another device waits until this device catches up with a device change."))
    #expect(one.contains { $0.hasPrefix("Edits wait 1 minute more") })
    fields["waiting"] = 2
    let two = try JSONDecoder().decode(SyncReport.self, from: JSONSerialization.data(withJSONObject: fields)).notes
    #expect(two.contains("2 edits wait for the sync server's clock to catch up. Nothing is lost."))
}

@Test func startFreshTurnsAnInstalledButOffCatalogBackOn() {
    let url = "https://v3-cinemeta.strem.io/manifest.json"
    func cinemeta(enabled: Bool) -> AddonInfo {
        AddonInfo(
            url: url, host: "v3-cinemeta.strem.io", name: "Cinemeta", description: nil, logo: nil, version: "3.0.14",
            enabled: enabled, provides: ["catalog", "meta"], types: ["movie", "series"], configurable: false, configureUrl: nil)
    }
    // Installed counts as installed and on: off is turned back on, never installed again or shown as done.
    #expect(AddonSetup.step(url, in: [cinemeta(enabled: false)]) == .turnOn)
    #expect(AddonSetup.step(url, in: [cinemeta(enabled: true)]) == .done)
    #expect(AddonSetup.step(url, in: []) == .install)
    // The list not read yet: install, and a core answering "already installed" has it turned on.
    #expect(AddonSetup.step(url, in: nil) == .install)
}

@Test func tvBackGoesHomeThenToTheTopThenExits() {
    // Another place, wherever it is scrolled: Home first.
    #expect(TVBack.step(atHome: false, scrolledDown: true) == .goHome)
    #expect(TVBack.step(atHome: false, scrolledDown: false) == .goHome)
    // Home down the rows: its top row; only Home at the top lets the app exit.
    #expect(TVBack.step(atHome: true, scrolledDown: true) == .toTop)
    #expect(TVBack.step(atHome: true, scrolledDown: false) == .exit)
}
