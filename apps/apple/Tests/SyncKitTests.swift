import Foundation
import Security
import SwiftUI
import Testing

// A recovery kit made here (Android's SyncAccount, round 2 reviews): held in
// memory until "I saved it", five minutes at most once its page is left,
// covered whenever the app leaves the front, and never claimed gone when
// nobody could name it. Only ids are ever kept.

private func member(_ id: String, kit: Bool = false, sponsor: String? = nil, this: Bool = false) -> MemberInfo {
    MemberInfo(id: id, kit: kit, name: nil, sponsor: sponsor, addedMs: 0, thisDevice: this)
}

@Suite struct SyncKitTests {
    @Test func aKitIsHeldUntilSavedAndOnlyItsIdIsKept() {
        var ledger = KitLedger()
        let kit = HeldKit(id: "k1", kit: "SECRET-KIT")
        ledger.hold(kit)
        #expect(ledger.held == kit)
        #expect(ledger.pending == ["k1"])
        // The held one is not unseen: it can still be shown.
        #expect(ledger.unseen.isEmpty && !ledger.unknownUnseen)
        // Printed or logged, a held kit never shows its letters.
        #expect(!"\(kit)".contains("SECRET"))
        ledger.saved()
        #expect(ledger.held == nil && ledger.pending.isEmpty)
    }

    @Test func aKitLeftUnsavedGoesUnseenAndTheNextOneDoesNotHideIt() {
        var ledger = KitLedger()
        let first = HeldKit(id: "k1", kit: "AAAA")
        ledger.hold(first)
        ledger.expire(first)
        #expect(ledger.held == nil && ledger.expired)
        #expect(ledger.unseen == ["k1"])
        // A later expiry of a kit no longer held changes nothing.
        let second = HeldKit(id: "k2", kit: "BBBB")
        ledger.hold(second)
        ledger.expire(first)
        #expect(ledger.held == second && !ledger.expired)
        #expect(ledger.unseen == ["k1"])
        // Removed (or gone from the list): it leaves the ledger.
        ledger.settle("k1")
        #expect(ledger.unseen.isEmpty && ledger.pending == ["k2"])
        // A launch after the app closed: pending ids come back, unseen.
        #expect(KitLedger(pending: ["k2"]).unseen == ["k2"])
    }

    @Test func aKitNobodyCouldNameIsNeverClaimedGone() {
        var ledger = KitLedger()
        let kit = HeldKit(id: "", kit: "CCCC")
        ledger.hold(kit)
        #expect(ledger.pending == [KitLedger.unknown])
        // While held, "I checked" keeps it pending.
        ledger.checkedUnknown()
        #expect(ledger.pending == [KitLedger.unknown])
        ledger.expire(kit)
        #expect(ledger.unknownUnseen && ledger.unseen.isEmpty)
        // A list without it cannot settle it: its id is not known.
        ledger.follow([member("a", this: true)])
        #expect(ledger.unknownUnseen)
        ledger.checkedUnknown()
        #expect(!ledger.unknownUnseen && ledger.pending.isEmpty)
    }

    @Test func aKitWhoseRowLeftTheListIsSettled() {
        var ledger = KitLedger(pending: ["k1", "k2"])
        ledger.follow([member("a", this: true), member("k2", kit: true, sponsor: "a")])
        #expect(ledger.pending == ["k2"])
        ledger.drop()
        #expect(ledger == KitLedger())
    }

    @Test func leavingTheFrontCoversTheKit() {
        // Phone: the app switcher, another app or the lock screen.
        #expect(KitRules.covers(.inactive, mac: false))
        #expect(KitRules.covers(.background, mac: false))
        #expect(!KitRules.covers(.active, mac: false))
        // Mac: a hidden or minimized window; not one beside a password manager.
        #expect(KitRules.covers(.background, mac: true))
        #expect(!KitRules.covers(.inactive, mac: true))
        #expect(!KitRules.covers(.active, mac: true))
    }

    @Test func anUnseenKitIsNoKit() {
        let members = [member("a", this: true), member("k1", kit: true, sponsor: "a")]
        #expect(!SyncRules.noKit(members, unseen: []))
        #expect(SyncRules.noKit(members, unseen: ["k1"]))
        #expect(!SyncRules.noKit(members + [member("k2", kit: true, sponsor: "a")], unseen: ["k1"]))
    }
}

/// The model's own hold, on a real core opened in a temporary directory.
@Suite(.serialized) @MainActor struct SyncKitModelTests {
    private func model() async throws -> (SyncModel, UserDefaults, RealCore) {
        let core = RealCore()
        #expect(await core.ready())
        let defaults = try #require(UserDefaults(suiteName: "mq-test-\(UUID().uuidString)"))
        let keychain = FakeKeychain()
        return (SyncModel(core: core.client, records: keychain.record, defaults: defaults), defaults, core)
    }

    @Test func theLettersGoFiveMinutesAfterThePageIsLeftAndTheKitIsThenUnseen() async throws {
        let (sync, defaults, core) = try await model()
        sync.kitHold = .milliseconds(150)
        sync.kits.hold(HeldKit(id: "k1", kit: "SECRET-KIT"))
        // Only the id is stored, never the letters.
        #expect(defaults.stringArray(forKey: "syncPendingKits") == ["k1"])
        sync.kitPage(open: true)
        try await Task.sleep(for: .milliseconds(400))
        #expect(sync.kits.held != nil, "never while the page can show it")
        sync.kitPage(open: false)
        // Back on the page before the hold runs out: it stays.
        sync.kitPage(open: true)
        try await Task.sleep(for: .milliseconds(400))
        #expect(sync.kits.held != nil)
        sync.kitPage(open: false)
        #expect(await eventually { sync.kits.held == nil })
        #expect(sync.kits.expired && sync.kits.unseen == ["k1"])
        // The next launch still knows it went unseen.
        #expect(SyncModel(core: core.client, records: FakeKeychain().record, defaults: defaults).kits.unseen == ["k1"])
        withExtendedLifetime(core) {}
    }

    @Test func aKitMadeWhileItsPageIsClosedStartsItsHold() async throws {
        let (sync, _, core) = try await model()
        sync.kitHold = .milliseconds(150)
        // The page was left (or another tab covers it) before the kit arrived.
        sync.kitPage(open: true)
        sync.kitPage(open: false)
        sync.hold(HeldKit(id: "", kit: "SECRET-KIT"))
        #expect(await eventually { sync.kits.held == nil })
        #expect(sync.kits.expired && sync.kits.unknownUnseen)
        // On its open page it stays.
        sync.kitPage(open: true)
        sync.hold(HeldKit(id: "k2", kit: "OTHER-KIT"))
        try await Task.sleep(for: .milliseconds(400))
        #expect(sync.kits.held?.id == "k2")
        withExtendedLifetime(core) {}
    }

    @Test func savingOrLeavingForgetsTheKit() async throws {
        let (sync, defaults, core) = try await model()
        sync.kits.hold(HeldKit(id: "k1", kit: "SECRET-KIT"))
        sync.kitSaved()
        #expect(sync.kits == KitLedger() && defaults.stringArray(forKey: "syncPendingKits") == [])
        sync.kits.hold(HeldKit(id: "k2", kit: "SECRET-KIT"))
        sync.forgetHere()
        #expect(sync.kits == KitLedger() && defaults.stringArray(forKey: "syncPendingKits") == [])
        withExtendedLifetime(core) {}
    }
}

/// A Keychain the test controls: what it holds, and whether it answers.
final class FakeKeychain: @unchecked Sendable {
    var held: JSONValue?
    var readFails = false
    var deleteFails = false

    init(_ held: JSONValue? = nil) {
        self.held = held
    }

    var record: SyncRecord {
        SyncRecord(
            load: { [self] in
                if readFails { throw KeychainError(status: errSecInteractionNotAllowed) }
                return held
            },
            store: { [self] in
                held = $0
                return true
            },
            delete: { [self] in
                guard !deleteFails else { return false }
                held = nil
                return true
            }
        )
    }
}

// Round 4 (reviewer r2): the gate that decides whether the kit shows, the kit
// named once the list shows it, and the owner check without a lock.
@Suite struct SyncKitGateTests {
    @Test func thePromptsOwnSceneChangeNeitherCoversNorAsksAgain() {
        var gate = KitGate()
        gate.ask()
        // On a device the app goes inactive under the prompt.
        gate.phase(.inactive, mac: false)
        gate.deactivating()
        gate.answered(ok: true)
        #expect(gate.unlocked && !gate.asking)
        // Out of the prompt, the same change covers it.
        gate.deactivating()
        #expect(!gate.unlocked)
        gate.ask()
        gate.answered(ok: true)
        gate.phase(.inactive, mac: false)
        #expect(!gate.unlocked)
    }

    @Test func aKitThatArrivesWhileItsPageWasCoveredStaysCovered() {
        var gate = KitGate()
        gate.ask()
        // Another tab while the server made the kit.
        gate.cover()
        gate.answered(ok: true)
        #expect(!gate.unlocked)
        // The background while the prompt was up.
        gate.ask()
        gate.phase(.background, mac: false)
        gate.answered(ok: true)
        #expect(!gate.unlocked)
        // A refused prompt shows nothing.
        gate.ask()
        gate.answered(ok: false)
        #expect(!gate.unlocked)
        // A new ask after a cover starts clean.
        gate.ask()
        gate.answered(ok: true)
        #expect(gate.unlocked)
    }

    @Test func aMacShowsTheKitForTwentySecondsOrWhileShowIsHeld() {
        var gate = KitGate()
        let t0 = Date(timeIntervalSince1970: 1_000)
        #expect(!gate.shows(now: t0, holding: true, mac: true))
        gate.ask()
        gate.answered(ok: true, now: t0)
        #expect(gate.shows(now: t0.addingTimeInterval(19), holding: false, mac: true))
        #expect(!gate.shows(now: t0.addingTimeInterval(20), holding: false, mac: true))
        // Held within a minute of the prompt it shows; after that only a new prompt does.
        #expect(gate.shows(now: t0.addingTimeInterval(59), holding: true, mac: true) && gate.canHold(now: t0.addingTimeInterval(59)))
        #expect(!gate.shows(now: t0.addingTimeInterval(61), holding: true, mac: true) && !gate.canHold(now: t0.addingTimeInterval(61)))
        gate.ask()
        gate.answered(ok: true, now: t0.addingTimeInterval(100))
        #expect(gate.shows(now: t0.addingTimeInterval(150), holding: true, mac: true))
        // A phone shows it for as long as the page stays in view.
        #expect(gate.shows(now: t0.addingTimeInterval(600), holding: false, mac: false))
        gate.cover()
        #expect(!gate.shows(now: t0, holding: true, mac: true))
        #expect(W.hiddenIn(1) == "Hides in 1 second" && W.hiddenIn(20) == "Hides in 20 seconds")
    }

    @Test func withoutALockOnlyStoppingSyncAsksTwiceAndRuns() {
        #expect(SyncRules.ownerStep(hasLock: true, endsAccess: true, askedAgain: false) == .prompt)
        #expect(SyncRules.ownerStep(hasLock: true, endsAccess: false, askedAgain: true) == .prompt)
        #expect(SyncRules.ownerStep(hasLock: false, endsAccess: true, askedAgain: false) == .askAgain)
        #expect(SyncRules.ownerStep(hasLock: false, endsAccess: true, askedAgain: true) == .run)
        #expect(SyncRules.ownerStep(hasLock: false, endsAccess: false, askedAgain: true) == .refuse)
    }
}

private typealias W = SyncWords
