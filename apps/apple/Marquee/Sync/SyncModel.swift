import Foundation
#if !os(tvOS)
import LocalAuthentication
#endif
import Observation

/// The sync account on this device: one per app. The keystore record lives
/// in the Keychain; every sync op gets it and may hand back a new one, which
/// is stored before anything else happens.
///
/// Every op on the record runs in turn (one at a time, in the order asked),
/// in tasks owned by this model, never by a view: leaving a page or a scene
/// change cannot cancel an op between the core's answer and storing its
/// record. A record that comes back is kept only if it is not older than the
/// one stored (`revision`). Rounds run in one worker: asking while a round
/// runs keeps one more for after it.
@MainActor @Observable
final class SyncModel {
    /// A record is stored, or held for this session: this device is in a sync account.
    private(set) var joined: Bool
    /// A record the Keychain kept that this device does not use: after
    /// leaving, or left by a deleted install. The page says so with Try again
    /// (`deleteKeyAgain`).
    private(set) var keyLeft: KeyLeft?

    enum KeyLeft: Equatable {
        case afterLeave, earlierInstall
    }
    /// The Keychain did not answer when asked for the record: this device may
    /// be in an account, so the page says so with Try again, never the ways in.
    private(set) var keyUnreadable = false
    /// A round runs or is asked for.
    private(set) var syncing = false
    private(set) var lastReport: SyncReport?
    private(set) var lastSynced: Date?
    private(set) var devices: SyncDevices?
    /// The last read of the device list failed: the list, and the 12
    /// characters with it, may be out of date until a read works again.
    private(set) var listFailed = false
    /// Changes that failed since this device joined, until the user dismisses them.
    private(set) var failed = SyncFailed()
    /// What went wrong in the last round, in words, and what the user can do about it.
    private(set) var problem: SyncProblem?
    /// What the last leave did, for the page that follows it.
    private(set) var notice: String?
    /// The pairing attempt. It belongs here, not to the page, so its join is
    /// stored the moment it arrives, whether or not the page is on screen.
    private(set) var pairing: LiveRecord<PairingSnapshot>?
    /// The last join: in the Keychain (true), for this session only (false), or none yet.
    private(set) var joinStored: Bool?
    /// Joined with a recovery kit and no new kit made since: the page asks for one.
    private(set) var recovered: Bool {
        didSet { defaults.set(recovered, forKey: "syncRecovered") }
    }

    /// Kits made here and not yet said saved: the one whose letters are held,
    /// and the ids of the pending ones (kept across launches, never a kit).
    /// Only this model changes it (tests seed it).
    var kits: KitLedger {
        didSet { if kits.pending != oldValue.pending { defaults.set(kits.pending, forKey: "syncPendingKits") } }
    }
    /// A kit is being made, in this model's own task: a page left meanwhile still gets it held.
    private(set) var makingKit = false

    @ObservationIgnored private let core: CoreClient
    @ObservationIgnored private let records: SyncRecord
    @ObservationIgnored private let defaults: UserDefaults
    /// How long a made kit's letters stay once its page is left.
    @ObservationIgnored var kitHold = KitLedger.hold
    @ObservationIgnored private var holdTimer: Task<Void, Never>?
    /// The kit page is in view (`kitPage`).
    @ObservationIgnored private var kitPageOpen = false
    @ObservationIgnored private var scheduled: Task<Void, Never>?
    @ObservationIgnored private var worker: Task<Void, Never>?
    @ObservationIgnored private var pending = false
    /// The last op in line; the next one starts when it ends.
    @ObservationIgnored private var line: Task<Void, Never>?
    /// Bumped by every join and leave. An op from before it stores and shows nothing.
    /// The main actor is the lock: the bump, the store and each op's check
    /// after its await never interleave.
    @ObservationIgnored private var account = 0
    /// A record the Keychain refused, held in memory: nothing is sent until it is stored.
    @ObservationIgnored private var unstored: JSONValue?

    /// `leftover`: a record left by a deleted install could not be removed yet
    /// (`SyncRecord.forgetLeftover`): it is never loaded, and the page offers
    /// to delete it.
    init(core: CoreClient, records: SyncRecord = .keychain, defaults: UserDefaults = .standard, leftover: Bool = false) {
        self.core = core
        self.records = records
        self.defaults = defaults
        recovered = defaults.bool(forKey: "syncRecovered")
        kits = KitLedger(pending: defaults.stringArray(forKey: "syncPendingKits") ?? [])
        if leftover {
            joined = false
            keyLeft = .earlierInstall
            return
        }
        do {
            joined = try records.load() != nil
        } catch {
            // Fails closed: a Keychain that did not answer is not "no account".
            joined = false
            keyUnreadable = true
            coreLog.error("sync: the Keychain did not give the record: \(error, privacy: .public)")
        }
    }

    private func refuseUnreadableKey() throws {
        if keyUnreadable { throw CoreError("the Keychain did not give the record", code: "keyUnreadable") }
    }

    /// Asks the Keychain for the record again (the page's Try again).
    func readKeyAgain() {
        do {
            joined = try records.load() != nil
            keyUnreadable = false
            if joined { request() }
        } catch {
            keyUnreadable = true
        }
    }

    // MARK: Rounds

    /// Asks for a round: now, or once the running one ends.
    func request() {
        pending = true
        guard worker == nil else { return }
        syncing = true
        worker = Task {
            while pending {
                pending = false
                await rounds()
            }
            syncing = false
            worker = nil
        }
    }

    /// Asks for a round a few seconds after an edit, so a burst of edits goes
    /// in one round. Cancelling cancels the wait only, never a round.
    func schedule() {
        guard joined else { return }
        scheduled?.cancel()
        scheduled = Task {
            try? await Task.sleep(for: .seconds(5))
            guard !Task.isCancelled else { return }
            request()
        }
    }

    /// Syncs, going again while the core asks to (a round that changed the
    /// record publishes nothing until it is stored).
    private func rounds() async {
        for _ in 0..<5 {
            guard joined else { return }
            let mine = account
            do {
                let report: SyncReport = try await withRecord("syncNow") { $0.record }
                // A join or a leave replaced the account while this round ran: its answer is not ours.
                guard account == mine else { return }
                lastReport = report
                lastSynced = .now
                problem = nil
                failed = failed.adding(report)
                // The 12 characters to compare come from the device list: keep it as fresh as the round.
                if report.fingerprint != devices?.fingerprint { try? await loadDevices() }
                if !report.again { return }
            } catch {
                guard account == mine else { return }
                problem = SyncProblem(error, action: .round)
                return
            }
        }
    }

    // MARK: Joining

    /// Starts pairing with a device that already syncs; a new attempt replaces the last.
    func startPairing(server: String, name: String) {
        // A Keychain that did not answer may hold an account: nothing replaces it unseen.
        guard !keyUnreadable else { return }
        let attempt = LiveRecord<PairingSnapshot>("pairing", params: ["server": server, "name": name], core: core)
        attempt.onSnapshot = { [weak self, weak attempt] snapshot in
            guard case .joined(let record) = snapshot, let self, let attempt, self.pairing === attempt, self.joinStored == nil else { return }
            self.join(record)
        }
        joinStored = nil
        pairing = attempt
        attempt.start()
    }

    /// Drops the attempt ("Try again", "New code").
    func endPairing() {
        pairing = nil
    }

    /// The first device of a new account.
    func createAccount(server: String, invite: String, name: String) async throws {
        try refuseUnreadableKey()
        var fields: [String: any Sendable] = ["server": server, "name": name]
        if !invite.isEmpty { fields["invite"] = invite }
        let record: JSONValue = try await core.call("createSyncAccount", fields, as: JSONValue.self)
        join(record)
    }

    /// A new device joining with a recovery kit; the page then asks for a new kit.
    func recover(server: String, kit: String, name: String) async throws {
        try refuseUnreadableKey()
        let record: JSONValue = try await core.call("recover", ["server": server, "kit": kit, "name": name], as: JSONValue.self)
        join(record, recovered: true)
    }

    /// A join replaces any account before it (the pages ask first), then its
    /// first round runs. When the Keychain refuses the record, the join lasts
    /// for this session and sync waits until the record is stored.
    private func join(_ record: JSONValue, recovered: Bool = false) {
        account += 1
        dropKits()
        // A new account's record is stored as it is: revisions compare only within an account.
        let stored = records.store(record)
        // A key left by a deleted install is gone once the new one took its place.
        if keyLeft == .earlierInstall, stored { SyncRecord.cleaned(defaults) }
        keyLeft = nil
        unstored = stored ? nil : record
        joinStored = stored
        joined = true
        notice = nil
        self.recovered = recovered
        lastReport = nil
        lastSynced = nil
        devices = nil
        listFailed = false
        failed = SyncFailed()
        problem = stored ? nil : SyncProblem(code: "storage", message: "")
        request()
    }

    // MARK: The account's devices

    /// The device list, fresh from the sync server.
    @discardableResult
    func loadDevices() async throws -> SyncDevices {
        let mine = account
        let list: SyncDevices
        do {
            list = try await withRecord("syncDevices") { $0.record }
        } catch {
            if account == mine, !(error is CancellationError) { listFailed = true }
            throw error
        }
        if account == mine {
            devices = list
            listFailed = false
            kits.follow(list.members)
        }
        return list
    }

    /// Vouches for the device that shows `qr` and hands it the account.
    /// Returns the six digits the new device must show too.
    /// Behind the OS prompt: whoever confirms the digits next joins the account.
    func approvePairing(_ qr: String) async throws -> String {
        try await confirmOwner(SyncWords.authApprove)
        struct Approved: Decodable, Sendable { let code: String; let record: JSONValue }
        let approved: Approved = try await withRecord("approvePairing", ["qr": qr]) { $0.record }
        // The new device joins once it confirms; the list catches up on the next look.
        try? await loadDevices()
        return approved.code
    }

    /// The digits on the new device differ: the approval of `qr` ends now, with
    /// a key rotation that drops the device if it joined meanwhile (and every
    /// device it let in). Behind the OS prompt; a cancelled prompt changes nothing.
    func cancelPairing(_ qr: String) async throws {
        try await confirmOwner(SyncWords.authDiffer)
        let _: JSONValue = try await withRecord("cancelPairing", ["qr": qr]) { $0 }
        try? await loadDevices()
    }

    /// Both sides of a different device list, read only.
    func forkPreview() async throws -> ForkPreview {
        try await withRecord("forkPreview") { (_: ForkPreview) in nil }
    }

    /// Makes a recovery kit and holds its letters here until "I saved it"
    /// (`kits`). Behind the OS prompt, as the protocol asks; never on a TV.
    /// The work runs in this model's own task: Cancel or Back while it runs
    /// still gets the kit held, and the hub says it waits. Unseen kits go
    /// first when the list shows them; one the list could not show stays.
    func createRecoveryKit() async throws {
        #if os(tvOS)
        throw CoreError("A TV does not make recovery kits.", code: "unconfirmed")
        #else
        guard !makingKit else { return }
        try await confirmOwner(SyncWords.authKit)
        makingKit = true
        try await Task { @MainActor in
            defer { makingKit = false }
            let mine = account
            let before = try? await loadDevices()
            if let kitsBefore = before.map({ Set($0.members.filter(\.kit).map(\.id)) }) {
                for id in kits.unseen {
                    if kitsBefore.contains(id) {
                        let _: JSONValue = try await withRecord("removeDevice", ["device": id]) { $0 }
                    }
                    kits.settle(id)
                }
            }
            struct Made: Decodable, Sendable { let kit: String; let record: JSONValue }
            let made: Made = try await withRecord("createRecoveryKit") { $0.record }
            guard mine == account else { return }
            recovered = false
            // Held at once, pending as unknown: an app stopped before the list answers still marks it.
            hold(HeldKit(id: "", kit: made.kit))
            let after = try? await loadDevices()
            guard mine == account else { return }
            kits.name(KitLedger.newKitId(before: before, after: after))
        }.value
        #endif
    }

    /// Shows the held kit again: only after the OS prompt.
    func confirmShowKit() async throws {
        try await confirmOwner(SyncWords.authShowKit)
    }

    /// The kit page is open, or was left: the letters never go while the
    /// page can show them, and go `kitHold` after it is left.
    func kitPage(open: Bool) {
        kitPageOpen = open
        holdTimer?.cancel()
        guard !open, let kit = kits.held else { return }
        // ponytail: a fixed hold after the page; the letters never outlive it, and the kit then counts as unseen.
        holdTimer = Task { [weak self, kitHold] in
            try? await Task.sleep(for: kitHold)
            guard !Task.isCancelled else { return }
            self?.kits.expire(kit)
        }
    }

    /// A kit just made: held, and when its page is not in view (left while
    /// the kit was made, or another tab over it) its hold starts now.
    func hold(_ kit: HeldKit) {
        kits.hold(kit)
        if !kitPageOpen { kitPage(open: false) }
    }

    /// "I saved it": the letters leave memory and the kit is no longer pending.
    func kitSaved() {
        holdTimer?.cancel()
        kits.saved()
    }

    /// The user checked the list for a kit nobody could name.
    func checkedUnknownKit() {
        kits.checkedUnknown()
    }

    private func dropKits() {
        holdTimer?.cancel()
        kits.drop()
    }

    /// What reporting `lost` would drop: its devices (ticked) and its kits (always).
    func lostPreview(_ lost: String) async throws -> LostPreview {
        try await withRecord("lostPreview", ["lost": lost]) { (_: LostPreview) in nil }
    }

    /// Removes a device or a kit. It has landed once this returns; a list that
    /// cannot be fetched right after does not undo it.
    func removeDevice(_ device: String) async throws {
        try await confirmOwner(SyncWords.authRemove)
        let _: JSONValue = try await withRecord("removeDevice", ["device": device]) { $0 }
        // A pending kit just removed needs no saving and no removing, list or no list.
        kits.settle(device)
        try? await loadDevices()
    }

    /// Drops `lost` and the devices it added except `keep`; their kits go too.
    func reportLost(_ lost: String, keep: [String]) async throws {
        try await confirmOwner(SyncWords.authLost)
        let _: JSONValue = try await withRecord("reportLost", ["lost": lost, "keep": keep]) { $0 }
        try? await loadDevices()
    }

    /// Leaves the account. Without `force` the core refuses while edits are unsent.
    func signOut(force: Bool, askedAgain: Bool = false) async throws {
        try await confirmOwner(SyncWords.authLeave, endsAccess: true, askedAgain: askedAgain)
        let _: JSONValue = try await withRecord("signOut", ["force": force]) { _ in nil }
        forget()
        problem = nil
        notice = SyncWords.left
    }

    /// Stops syncing on this device only: nothing is sent and the sync server
    /// still lists it; the core takes only this device's own record
    /// (`wrongRecord` otherwise). The way out when the server will not let it leave.
    func leaveHere(askedAgain: Bool = false) async throws {
        try await confirmOwner(SyncWords.authLeave, endsAccess: true, askedAgain: askedAgain)
        let _: JSONValue = try await withRecord("signOut", ["force": true, "local": true]) { _ in nil }
        forget()
        problem = nil
        notice = SyncWords.leftHere
    }

    /// After `fork`: the sync server's device list, as the compare page showed
    /// it (`head`), without the devices the user ticked. A list that moved since
    /// is refused with `rosterChanged`.
    func useServerRoster(drop: [String], head: String) async throws {
        try await confirmOwner(SyncWords.authServerList)
        let _: JSONValue = try await withRecord("useServerRoster", ["drop": drop, "head": head]) { $0 }
        problem = nil
        request()
        try? await loadDevices()
    }

    /// Forgets the failures listed since joining.
    func dismissFailed() {
        failed = SyncFailed()
    }

    /// Deletes a record the Keychain kept (`keyLeft`).
    func deleteKeyAgain() {
        guard let left = keyLeft, records.delete() else { return }
        if left == .earlierInstall { SyncRecord.cleaned(defaults) }
        keyLeft = nil
    }

    /// Drops the account record here only, when the relay cannot take this
    /// device back (wrong relay, entries lost). The library stays.
    func forgetHere() {
        forget()
        problem = nil
    }

    // MARK: The record

    /// Runs one op on the record, in turn. A record in the answer is stored
    /// before the next op runs; `dropped` (this device was removed) forgets
    /// the account here.
    private func withRecord<R: Decodable & Sendable>(
        _ op: String, _ fields: [String: any Sendable] = [:], record recordIn: @escaping @Sendable (R) -> JSONValue?
    ) async throws -> R {
        try await inTurn { [self] in
            let mine = account
            guard let record = try storedRecord() else { throw CoreError("this device has not joined the account", code: "notJoined") }
            var all = fields
            all["record"] = record
            do {
                let result: R = try await core.call(op, all, as: R.self)
                if let next = recordIn(result), mine == account, !keep(next) {
                    throw CoreError("the record was not stored", code: "storage")
                }
                return result
            } catch let error as CoreError where mine == account && (error.code == "dropped" || error.code == "fork") {
                if error.code == "dropped" { forget() }
                problem = SyncProblem(error, action: .round)
                throw error
            }
        }
    }

    /// One op after another, in the order asked. The work runs in its own
    /// task, so a caller that goes away does not stop it half way.
    private func inTurn<T: Sendable>(_ work: @escaping @MainActor () async throws -> T) async throws -> T {
        let before = line
        let run = Task { @MainActor in
            await before?.value
            return try await work()
        }
        line = Task { _ = await run.result }
        return try await run.value
    }

    /// The account's record from the Keychain. One held in memory is stored
    /// first, and while the Keychain refuses it nothing is sent.
    private func storedRecord() throws -> JSONValue? {
        if let held = unstored {
            guard records.store(held) else {
                problem = SyncProblem(code: "storage", message: "")
                throw CoreError("the record was not stored", code: "storage")
            }
            unstored = nil
        }
        do {
            return try records.load()
        } catch {
            throw CoreError("the Keychain did not give the record", code: "keyUnreadable")
        }
    }

    /// Stores a record the core handed back, unless the stored one is newer.
    /// When the Keychain refuses it, it is held here and sync pauses until it is stored.
    private func keep(_ next: JSONValue) -> Bool {
        // A stored record that does not read is never overwritten blind: the new one waits here.
        let stored: JSONValue?
        do {
            stored = try records.load()
        } catch {
            unstored = next
            problem = SyncProblem(code: "storage", message: "")
            return false
        }
        let record = SyncRules.newer(stored, next)
        guard records.store(record) else {
            unstored = record
            problem = SyncProblem(code: "storage", message: "")
            return false
        }
        unstored = nil
        return true
    }

    /// Forgets the account here. When the Keychain refuses to delete the
    /// record, `keyLeft` says so: the next launch would otherwise find it.
    private func forget() {
        account += 1
        keyLeft = records.delete() ? nil : .afterLeave
        if keyLeft != nil { coreLog.error("sync: the Keychain kept the record after leaving") }
        unstored = nil
        joined = false
        joinStored = nil
        pairing = nil
        devices = nil
        listFailed = false
        failed = SyncFailed()
        lastReport = nil
        lastSynced = nil
        recovered = false
        dropKits()
    }

    /// The OS prompt (passcode, Face ID or Touch ID) before changing the
    /// account's devices or making a kit, as the protocol asks. A TV has no
    /// way to check who is there: its pages ask a second time instead.
    /// `endsAccess`: the op only ends this device's access (stopping syncing):
    /// with no lock it is not refused, the page asks a second time
    /// (`noLockAsk`) and calls again with `askedAgain`.
    private func confirmOwner(_ reason: String, endsAccess: Bool = false, askedAgain: Bool = false) async throws {
        #if os(tvOS)
        // Other TV pages ask a second time on their own.
        let hasLock = !endsAccess
        #else
        let context = LAContext()
        var error: NSError?
        let hasLock = context.canEvaluatePolicy(.deviceOwnerAuthentication, error: &error)
        #endif
        switch SyncRules.ownerStep(hasLock: hasLock, endsAccess: endsAccess, askedAgain: askedAgain) {
        case .prompt:
            #if !os(tvOS)
            let ok = (try? await context.evaluatePolicy(.deviceOwnerAuthentication, localizedReason: reason)) ?? false
            guard ok else { throw CoreError("not confirmed", code: "unconfirmed") }
            #endif
        case .run: return
        case .askAgain: throw CoreError("no lock", code: "noLockAsk")
        case .refuse: throw CoreError("no passcode", code: "noPasscode")
        }
    }
}
